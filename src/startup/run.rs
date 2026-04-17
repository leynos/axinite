//! Agent run and shutdown orchestration after startup has completed.

use std::sync::Arc;
use std::time::Duration;

use ironclaw::{
    agent::{Agent, AgentDeps},
    app::AppComponents,
    channels::web::types::SseEvent,
    config::Config,
    context::ContextManager,
    orchestrator::{ReaperConfig, SandboxReaper},
};

use crate::startup::wasm::WasmWiringContext;
use crate::startup::{
    CoreAgentContext, GatewayPhaseContext, run_flow::run_with_side_effects,
    wasm::wire_wasm_channel_runtime,
};

/// Runs the agent loop and performs the coordinated shutdown sequence on exit.
///
/// Wires WASM channel runtime, snapshots workspace memory if a recording handle
/// is present, spawns the optional sandbox reaper, registers the SIGHUP
/// hot-reload handler on Unix, then calls `agent.run().await`. After the agent
/// exits the function broadcasts shutdown, flushes traces, stops the webhook
/// server, and tears down any active tunnel.
pub(crate) async fn run_agent(ctx: GatewayPhaseContext) -> anyhow::Result<()> {
    let GatewayPhaseContext {
        core:
            CoreAgentContext {
                config,
                components,
                side_effects,
                active_tunnel,
                container_job_manager,
                ..
            },
        channels,
        webhook_server,
        mut loaded_wasm_channel_names,
        mut wasm_channel_runtime_state,
        #[cfg(unix)]
        http_channel_state,
        session_manager,
        scheduler_slot,
        sse_sender,
        routine_engine_slot,
        ..
    } = ctx;

    prepare_channels(ChannelPreparation {
        components: &components,
        extension_manager: &components.extension_manager,
        wasm_channel_runtime_state: &mut wasm_channel_runtime_state,
        loaded_wasm_channel_names: &mut loaded_wasm_channel_names[..],
        channels: &channels,
        sse_sender: &sse_sender,
        wasm_channel_owner_ids: &config.channels.wasm_channel_owner_ids,
    })
    .await;

    snapshot_workspace_memory(&components).await;

    let agent = prepare_agent(
        &components,
        &config,
        AgentSetupResources {
            channels: Arc::clone(&channels),
            session_manager,
            scheduler_slot,
            sse_sender,
            routine_engine_slot,
        },
    )
    .await;

    let shutdown_tx = setup_runtime_management(
        &components,
        &config,
        &container_job_manager,
        #[cfg(unix)]
        &webhook_server,
        #[cfg(unix)]
        &http_channel_state,
    );

    let run_result = run_with_side_effects(side_effects, agent, || async {
        run_shutdown_sequence(
            &shutdown_tx,
            &components.mcp_process_manager,
            &components.recording_handle,
            &webhook_server,
            &active_tunnel,
        )
        .await;
    })
    .await;

    if run_result.is_ok() {
        run_shutdown_sequence(
            &shutdown_tx,
            &components.mcp_process_manager,
            &components.recording_handle,
            &webhook_server,
            &active_tunnel,
        )
        .await;
    }

    run_result?;
    Ok(())
}

/// Groups the runtime pieces needed to register message tools and wire the
/// loaded WASM channel runtime back into the live channel registry.
///
/// The borrowed fields all come from `run_agent`'s startup context and must
/// stay valid for the duration of `prepare_channels`.
struct ChannelPreparation<'a> {
    components: &'a AppComponents,
    extension_manager: &'a Option<Arc<ironclaw::extensions::ExtensionManager>>,
    wasm_channel_runtime_state: &'a mut Option<crate::startup::wasm::WasmChannelRuntimeState>,
    loaded_wasm_channel_names: &'a mut [String],
    channels: &'a Arc<ironclaw::channels::ChannelManager>,
    sse_sender: &'a Option<tokio::sync::broadcast::Sender<SseEvent>>,
    wasm_channel_owner_ids: &'a std::collections::HashMap<String, i64>,
}

/// Registers channel-backed tools and hands the loaded WASM runtime state back
/// to the extension manager.
///
/// This must run before the agent starts so startup-only channel state is fully
/// available to message tools.
async fn prepare_channels(preparation: ChannelPreparation<'_>) {
    let ChannelPreparation {
        components,
        extension_manager,
        wasm_channel_runtime_state,
        loaded_wasm_channel_names,
        channels,
        sse_sender,
        wasm_channel_owner_ids,
    } = preparation;
    components
        .tools
        .register_message_tools(Arc::clone(channels))
        .await;

    wire_wasm_channel_runtime(
        &WasmWiringContext {
            extension_manager,
            channels,
            sse_sender,
            wasm_channel_owner_ids,
        },
        wasm_channel_runtime_state,
        loaded_wasm_channel_names,
    )
    .await;
}

/// Persists an initial workspace-memory snapshot when both the recorder and
/// workspace are available.
///
/// The snapshot happens before the agent loop so recordings capture the
/// pre-run workspace state.
async fn snapshot_workspace_memory(components: &AppComponents) {
    if let Some(ref recorder) = components.recording_handle
        && let Some(ref ws) = components.workspace
    {
        recorder.snapshot_memory(ws).await;
    }
}

/// Bundles the runtime-owned resources needed to finish agent construction
/// after channel preparation.
///
/// The contained scheduler and routine-engine slots are populated during
/// `prepare_agent` and then handed back to the wider startup pipeline.
struct AgentSetupResources {
    channels: Arc<ironclaw::channels::ChannelManager>,
    session_manager: Arc<ironclaw::agent::SessionManager>,
    scheduler_slot: ironclaw::tools::builtin::SchedulerSlot,
    sse_sender: Option<tokio::sync::broadcast::Sender<ironclaw::channels::web::types::SseEvent>>,
    routine_engine_slot: Option<ironclaw::channels::web::server::RoutineEngineSlot>,
}

/// Builds the agent, publishes its scheduler handle, and wires any routine
/// engine slot before the run loop starts.
///
/// This function consumes the setup resources because the agent takes
/// ownership of the runtime connections they contain.
async fn prepare_agent(
    components: &AppComponents,
    config: &Config,
    resources: AgentSetupResources,
) -> Agent {
    let AgentSetupResources {
        channels,
        session_manager,
        scheduler_slot,
        sse_sender,
        routine_engine_slot,
    } = resources;
    let deps = build_agent_deps(components, config, sse_sender);
    let mut agent = build_agent(
        config,
        deps,
        AgentConnections {
            channels,
            session_manager,
            context_manager: Arc::clone(&components.context_manager),
        },
    );

    *scheduler_slot.write().await = Some(agent.scheduler());

    if let Some(slot) = routine_engine_slot {
        agent.set_routine_engine_slot(slot);
    }

    agent
}

/// Starts background runtime-management tasks and returns the shared shutdown
/// broadcaster used during teardown.
///
/// Common setup runs on every platform, while Unix-specific hot-reload wiring
/// is delegated to `setup_runtime_management_unix`.
pub(crate) fn setup_runtime_management(
    components: &AppComponents,
    config: &Config,
    container_job_manager: &Option<Arc<ironclaw::orchestrator::ContainerJobManager>>,
    #[cfg(unix)] webhook_server: &Option<
        Arc<tokio::sync::Mutex<ironclaw::channels::WebhookServer>>,
    >,
    #[cfg(unix)] http_channel_state: &Option<Arc<ironclaw::channels::HttpChannelState>>,
) -> tokio::sync::broadcast::Sender<()> {
    let reaper_context_manager = Arc::clone(&components.context_manager);
    maybe_spawn_sandbox_reaper(container_job_manager, reaper_context_manager, config);

    let (shutdown_tx, _) = tokio::sync::broadcast::channel::<()>(1);

    #[cfg(unix)]
    crate::startup::unix_runtime::setup_runtime_management_unix(
        components,
        webhook_server,
        http_channel_state,
        &shutdown_tx,
    );

    shutdown_tx
}

/// Derives the `AgentDeps` bundle from the built application components and
/// current runtime configuration.
///
/// Middleware instances are created here so the agent receives a fully wired
/// dependency graph before construction.
fn build_agent_deps(
    components: &AppComponents,
    config: &Config,
    sse_sender: Option<tokio::sync::broadcast::Sender<SseEvent>>,
) -> AgentDeps {
    let http_interceptor = components
        .recording_handle
        .as_ref()
        .map(|r| r.http_interceptor());

    AgentDeps {
        store: components.db.clone(),
        llm: Arc::clone(&components.llm),
        cheap_llm: components.cheap_llm.clone(),
        safety: Arc::clone(&components.safety),
        tools: Arc::clone(&components.tools),
        workspace: components.workspace.clone(),
        extension_manager: components.extension_manager.clone(),
        skill_registry: components.skill_registry.clone(),
        skill_catalog: components.skill_catalog.clone(),
        skills_config: config.skills.clone(),
        hooks: Arc::clone(&components.hooks),
        cost_guard: Arc::clone(&components.cost_guard),
        sse_tx: sse_sender,
        http_interceptor,
        transcription: config
            .transcription
            .create_provider()
            .map(|p| Arc::new(ironclaw::transcription::TranscriptionMiddleware::new(p))),
        document_extraction: Some(Arc::new(
            ironclaw::document_extraction::DocumentExtractionMiddleware::new(),
        )),
    }
}

/// Collects the long-lived runtime connections that `Agent::new` consumes.
///
/// These handles must already be initialized by earlier startup phases and are
/// moved into the final agent during `build_agent`.
struct AgentConnections {
    channels: Arc<ironclaw::channels::ChannelManager>,
    session_manager: Arc<ironclaw::agent::SessionManager>,
    context_manager: Arc<ironclaw::context::ContextManager>,
}

/// Constructs the final `Agent` from the prepared dependency bundle and live
/// runtime connections.
///
/// The returned agent is ready to enter the run loop once startup side effects
/// have been started.
fn build_agent(config: &Config, deps: AgentDeps, connections: AgentConnections) -> Agent {
    let AgentConnections {
        channels,
        session_manager,
        context_manager,
    } = connections;
    Agent::new(
        config.agent.clone(),
        deps,
        channels,
        Some(config.heartbeat.clone()),
        Some(config.hygiene.clone()),
        Some(config.routines.clone()),
        Some(context_manager),
        Some(session_manager),
    )
}

/// Spawns the sandbox reaper task when container-job management is enabled.
///
/// The reaper runs independently on the Tokio runtime and logs initialization
/// failures rather than aborting startup.
pub(crate) fn maybe_spawn_sandbox_reaper(
    container_job_manager: &Option<Arc<ironclaw::orchestrator::ContainerJobManager>>,
    reaper_context_manager: Arc<ContextManager>,
    config: &Config,
) {
    if let Some(jm) = container_job_manager {
        let reaper_jm = Arc::clone(jm);
        let reaper_config = ReaperConfig {
            scan_interval: Duration::from_secs(config.sandbox.reaper_interval_secs),
            orphan_threshold: Duration::from_secs(config.sandbox.orphan_threshold_secs),
            ..ReaperConfig::default()
        };
        let reaper_ctx = Arc::clone(&reaper_context_manager);
        tokio::spawn(async move {
            match SandboxReaper::new(reaper_jm, reaper_ctx, reaper_config).await {
                Ok(reaper) => reaper.run().await,
                Err(e) => tracing::error!("Sandbox reaper failed to initialize: {e}"),
            }
        });
    }
}

/// Performs the ordered shutdown sequence for background runtime pieces after
/// the agent loop exits.
///
/// Shutdown broadcasting happens first so listeners can stop before traces,
/// webhook state, and tunnels are flushed or torn down.
async fn run_shutdown_sequence(
    shutdown_tx: &tokio::sync::broadcast::Sender<()>,
    mcp_process_manager: &ironclaw::tools::mcp::McpProcessManager,
    recording_handle: &Option<Arc<ironclaw::llm::recording::RecordingLlm>>,
    webhook_server: &Option<Arc<tokio::sync::Mutex<ironclaw::channels::WebhookServer>>>,
    active_tunnel: &Option<Box<dyn ironclaw::tunnel::Tunnel>>,
) {
    let _ = shutdown_tx.send(());
    mcp_process_manager.shutdown_all().await;

    if let Some(recorder) = recording_handle
        && let Err(e) = recorder.flush().await
    {
        tracing::warn!("Failed to write LLM trace: {e}");
    }

    if let Some(ws_arc) = webhook_server {
        ws_arc.lock().await.shutdown().await;
    }

    if let Some(tunnel) = active_tunnel {
        tracing::debug!("Stopping {} tunnel...", tunnel.name());
        if let Err(e) = tunnel.stop().await {
            tracing::warn!("Failed to stop tunnel cleanly: {e}");
        }
    }

    tracing::debug!("Agent shutdown complete");
}

#[cfg(test)]
#[path = "run_tests.rs"]
mod tests;
