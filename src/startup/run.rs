//! Agent run and shutdown orchestration after startup has completed.

use std::sync::Arc;
use std::time::Duration;

use ironclaw::{
    agent::{Agent, AgentDeps},
    app::AppComponents,
    channels::{WebhookServer, web::types::SseEvent},
    config::Config,
    context::ContextManager,
    orchestrator::{ReaperConfig, SandboxReaper},
};

#[cfg(unix)]
use ironclaw::{channels::HttpChannelState, secrets::SecretsStore};

use crate::startup::{CoreAgentContext, GatewayPhaseContext, wasm::wire_wasm_channel_runtime};

#[cfg(unix)]
use crate::startup::channels::spawn_sighup_handler;

/// Runs the agent loop and performs the coordinated shutdown sequence on exit.
///
/// Wires WASM channel runtime, snapshots workspace memory if a recording handle
/// is present, spawns the optional sandbox reaper, registers the SIGHUP
/// hot-reload handler on Unix, then calls `agent.run().await`. After the agent
/// exits the function broadcasts shutdown, flushes traces, stops the webhook
/// server, and tears down any active tunnel.
pub(crate) async fn run_agent(ctx: GatewayPhaseContext) -> anyhow::Result<()> {
    let GatewayPhaseContext {
        core,
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
    let CoreAgentContext {
        config,
        components,
        side_effects,
        active_tunnel,
        container_job_manager,
        ..
    } = core;

    let channels = Arc::new(channels);
    prepare_channels(
        &components,
        &components.extension_manager,
        &mut wasm_channel_runtime_state,
        &mut loaded_wasm_channel_names[..],
        &channels,
        &sse_sender,
        &config.channels.wasm_channel_owner_ids,
    )
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
        &webhook_server,
        #[cfg(unix)]
        &http_channel_state,
    );

    side_effects.start()?;
    let run_result = agent.run().await;

    run_shutdown_sequence(
        &shutdown_tx,
        &components.mcp_process_manager,
        &components.recording_handle,
        &webhook_server,
        &active_tunnel,
    )
    .await;

    run_result?;
    Ok(())
}

async fn prepare_channels(
    components: &AppComponents,
    extension_manager: &Option<Arc<ironclaw::extensions::ExtensionManager>>,
    wasm_channel_runtime_state: &mut Option<crate::startup::wasm::WasmChannelRuntimeState>,
    loaded_wasm_channel_names: &mut [String],
    channels: &Arc<ironclaw::channels::ChannelManager>,
    sse_sender: &Option<tokio::sync::broadcast::Sender<SseEvent>>,
    wasm_channel_owner_ids: &std::collections::HashMap<String, i64>,
) {
    components
        .tools
        .register_message_tools(Arc::clone(channels))
        .await;

    wire_wasm_channel_runtime(
        extension_manager,
        wasm_channel_runtime_state,
        loaded_wasm_channel_names,
        channels,
        sse_sender,
        wasm_channel_owner_ids,
    )
    .await;
}

async fn snapshot_workspace_memory(components: &AppComponents) {
    if let Some(ref recorder) = components.recording_handle
        && let Some(ref ws) = components.workspace
    {
        recorder.snapshot_memory(ws).await;
    }
}

struct AgentSetupResources {
    channels: Arc<ironclaw::channels::ChannelManager>,
    session_manager: Arc<ironclaw::agent::SessionManager>,
    scheduler_slot: ironclaw::tools::builtin::SchedulerSlot,
    sse_sender: Option<tokio::sync::broadcast::Sender<ironclaw::channels::web::types::SseEvent>>,
    routine_engine_slot: Option<ironclaw::channels::web::server::RoutineEngineSlot>,
}

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

#[cfg_attr(not(unix), allow(unused_variables))]
fn setup_runtime_management(
    components: &AppComponents,
    config: &Config,
    container_job_manager: &Option<Arc<ironclaw::orchestrator::ContainerJobManager>>,
    webhook_server: &Option<Arc<tokio::sync::Mutex<WebhookServer>>>,
    #[cfg(unix)] http_channel_state: &Option<Arc<HttpChannelState>>,
) -> tokio::sync::broadcast::Sender<()> {
    let reaper_context_manager = Arc::clone(&components.context_manager);
    maybe_spawn_sandbox_reaper(container_job_manager, reaper_context_manager, config);

    let (shutdown_tx, _) = tokio::sync::broadcast::channel::<()>(1);

    #[cfg(unix)]
    {
        let sighup_settings_store: Option<Arc<dyn ironclaw::db::SettingsStore>> = components
            .db
            .as_ref()
            .map(|db| Arc::clone(db) as Arc<dyn ironclaw::db::SettingsStore>);

        setup_sighup_reload(
            sighup_settings_store,
            webhook_server,
            components.secrets_store.clone(),
            http_channel_state,
            &shutdown_tx,
        );
    }

    shutdown_tx
}

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

struct AgentConnections {
    channels: Arc<ironclaw::channels::ChannelManager>,
    session_manager: Arc<ironclaw::agent::SessionManager>,
    context_manager: Arc<ironclaw::context::ContextManager>,
}

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

fn maybe_spawn_sandbox_reaper(
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

#[cfg(unix)]
fn setup_sighup_reload(
    sighup_settings_store: Option<Arc<dyn ironclaw::db::SettingsStore>>,
    webhook_server: &Option<Arc<tokio::sync::Mutex<WebhookServer>>>,
    secrets_store: Option<Arc<dyn SecretsStore + Send + Sync>>,
    http_channel_state: &Option<Arc<HttpChannelState>>,
    shutdown_tx: &tokio::sync::broadcast::Sender<()>,
) {
    use ironclaw::channels::ChannelSecretUpdater;

    let mut secret_updaters: Vec<Arc<dyn ChannelSecretUpdater>> = Vec::new();
    if let Some(state) = http_channel_state {
        secret_updaters.push(Arc::clone(state) as Arc<dyn ChannelSecretUpdater>);
    }
    let reload_manager = Arc::new(ironclaw::reload::create_hot_reload_manager(
        sighup_settings_store.clone(),
        webhook_server.clone(),
        secrets_store,
        secret_updaters,
    ));
    spawn_sighup_handler(reload_manager, shutdown_tx);
}

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
