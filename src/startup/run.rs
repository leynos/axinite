//! Agent run and shutdown orchestration after startup has completed.

use std::sync::Arc;
use std::time::Duration;

use ironclaw::{
    agent::{Agent, AgentDeps},
    orchestrator::{ReaperConfig, SandboxReaper},
};

use crate::startup::{
    channels::spawn_sighup_handler, phases::GatewayPhaseContext, wasm::wire_wasm_channel_runtime,
};

pub(crate) async fn run_agent(ctx: GatewayPhaseContext) -> anyhow::Result<()> {
    let GatewayPhaseContext {
        config,
        components,
        side_effects,
        active_tunnel,
        container_job_manager,
        prompt_queue: _prompt_queue,
        docker_status: _docker_status,
        log_broadcaster: _log_broadcaster,
        log_level_handle: _log_level_handle,
        job_event_tx: _job_event_tx,
        channels,
        webhook_server,
        channel_names: _channel_names,
        mut loaded_wasm_channel_names,
        mut wasm_channel_runtime_state,
        #[cfg(unix)]
        http_channel_state,
        session_manager,
        scheduler_slot,
        gateway_url: _gateway_url,
        sse_sender,
        routine_engine_slot,
    } = ctx;

    let channels = Arc::new(channels);
    components
        .tools
        .register_message_tools(Arc::clone(&channels))
        .await;

    wire_wasm_channel_runtime(
        &components.extension_manager,
        &mut wasm_channel_runtime_state,
        &mut loaded_wasm_channel_names[..],
        &channels,
        &sse_sender,
        &config.channels.wasm_channel_owner_ids,
    )
    .await;

    if let Some(ref recorder) = components.recording_handle
        && let Some(ref ws) = components.workspace
    {
        recorder.snapshot_memory(ws).await;
    }

    let http_interceptor = components
        .recording_handle
        .as_ref()
        .map(|r| r.http_interceptor());
    let reaper_context_manager = Arc::clone(&components.context_manager);

    #[cfg(unix)]
    let sighup_settings_store: Option<Arc<dyn ironclaw::db::SettingsStore>> = components
        .db
        .as_ref()
        .map(|db| Arc::clone(db) as Arc<dyn ironclaw::db::SettingsStore>);

    let deps = AgentDeps {
        store: components.db,
        llm: components.llm,
        cheap_llm: components.cheap_llm,
        safety: components.safety,
        tools: components.tools,
        workspace: components.workspace,
        extension_manager: components.extension_manager,
        skill_registry: components.skill_registry,
        skill_catalog: components.skill_catalog,
        skills_config: config.skills.clone(),
        hooks: components.hooks,
        cost_guard: components.cost_guard,
        sse_tx: sse_sender,
        http_interceptor,
        transcription: config
            .transcription
            .create_provider()
            .map(|p| Arc::new(ironclaw::transcription::TranscriptionMiddleware::new(p))),
        document_extraction: Some(Arc::new(
            ironclaw::document_extraction::DocumentExtractionMiddleware::new(),
        )),
    };

    let mut agent = Agent::new(
        config.agent.clone(),
        deps,
        channels,
        Some(config.heartbeat.clone()),
        Some(config.hygiene.clone()),
        Some(config.routines.clone()),
        Some(components.context_manager),
        Some(session_manager),
    );

    *scheduler_slot.write().await = Some(agent.scheduler());

    if let Some(ref jm) = container_job_manager {
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

    if let Some(slot) = routine_engine_slot {
        agent.set_routine_engine_slot(slot);
    }

    let (shutdown_tx, _) = tokio::sync::broadcast::channel::<()>(1);

    #[cfg(unix)]
    {
        use ironclaw::channels::ChannelSecretUpdater;

        let mut secret_updaters: Vec<Arc<dyn ChannelSecretUpdater>> = Vec::new();
        if let Some(ref state) = http_channel_state {
            secret_updaters.push(Arc::clone(state) as Arc<dyn ChannelSecretUpdater>);
        }
        let reload_manager = Arc::new(ironclaw::reload::create_hot_reload_manager(
            sighup_settings_store.clone(),
            webhook_server.clone(),
            components.secrets_store.clone(),
            secret_updaters,
        ));
        spawn_sighup_handler(reload_manager, &shutdown_tx);
    }

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
