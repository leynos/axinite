//! Startup wiring for the gateway web-UI channel.

use std::{
    net::{IpAddr, SocketAddr},
    sync::Arc,
};

use axinite::{
    app::AppComponents,
    channels::{ChannelManager, GatewayChannel, web::log_layer::LogBroadcaster},
    config::Config,
};

/// Runtime-service dependencies required to configure the gateway channel.
pub(crate) struct GatewayContext<'a> {
    /// Environment snapshot resolved during startup configuration loading.
    pub(crate) env_context: &'a axinite::config::EnvContext,
    /// Container job manager exposed to the gateway for sandbox operations.
    pub(crate) container_job_manager: &'a Option<Arc<axinite::orchestrator::ContainerJobManager>>,
    /// Session manager used by gateway sessions.
    pub(crate) session_manager: &'a Arc<axinite::agent::SessionManager>,
    /// Log broadcaster backing live gateway log streaming.
    pub(crate) log_broadcaster: &'a Arc<LogBroadcaster>,
    /// Shared log-level handle used by the gateway UI.
    pub(crate) log_level_handle: &'a Arc<axinite::channels::web::log_layer::LogLevelHandle>,
    /// Prompt queue shared with sandbox-backed job interactions.
    pub(crate) prompt_queue: &'a Arc<
        tokio::sync::Mutex<
            std::collections::HashMap<
                uuid::Uuid,
                std::collections::VecDeque<axinite::orchestrator::api::PendingPrompt>,
            >,
        >,
    >,
    /// Scheduler slot injected into the gateway after startup.
    pub(crate) scheduler_slot: &'a axinite::tools::builtin::SchedulerSlot,
    /// Broadcast sender for relaying job events into the gateway.
    pub(crate) job_event_tx: &'a Option<
        tokio::sync::broadcast::Sender<(uuid::Uuid, axinite::channels::web::types::SseEvent)>,
    >,
    /// Live channel manager used to register the gateway channel.
    pub(crate) channels: &'a ChannelManager,
    /// Mutable list of enabled channel names displayed on the boot screen.
    pub(crate) channel_names: &'a mut Vec<String>,
}

/// Values produced by [`setup_gateway_channel`] and later threaded into
/// [`GatewayPhaseContext`].
pub(crate) struct GatewaySetup {
    /// Computed web-UI URL including auth token; `None` when the gateway is not
    /// configured.
    pub(crate) gateway_url: Option<String>,
    /// Gateway SSE broadcast sender for pushing events to connected clients.
    pub(crate) sse_sender:
        Option<tokio::sync::broadcast::Sender<axinite::channels::web::types::SseEvent>>,
    /// Slot for injecting a routine engine into the gateway after startup.
    pub(crate) routine_engine_slot: Option<axinite::channels::web::server::RoutineEngineSlot>,
}

fn configure_gateway_builder(
    mut gw: GatewayChannel,
    components: &AppComponents,
    ctx: &GatewayContext<'_>,
) -> GatewayChannel {
    gw = gw.with_llm_provider(Arc::clone(&components.llm));
    if let Some(ref ws) = components.workspace {
        gw = gw.with_workspace(Arc::clone(ws));
    }
    gw = gw.with_session_manager(Arc::clone(ctx.session_manager));
    gw = gw.with_log_broadcaster(Arc::clone(ctx.log_broadcaster));
    gw = gw.with_log_level_handle(Arc::clone(ctx.log_level_handle));
    gw = gw.with_tool_registry(Arc::clone(&components.tools));
    if let Some(ref ext_mgr) = components.extension_manager {
        gw = gw.with_extension_manager(Arc::clone(ext_mgr));
    }
    if !components.catalog_entries.is_empty() {
        gw = gw.with_registry_entries(components.catalog_entries.clone());
    }
    if let Some(ref d) = components.db {
        gw = gw.with_store(Arc::clone(d));
    }
    if let Some(jm) = ctx.container_job_manager {
        gw = gw.with_job_manager(Arc::clone(jm));
    }
    gw = gw.with_scheduler(ctx.scheduler_slot.clone());
    if let Some(ref sr) = components.skill_registry {
        gw = gw.with_skill_registry(Arc::clone(sr));
    }
    if let Some(ref sc) = components.skill_catalog {
        gw = gw.with_skill_catalog(Arc::clone(sc));
    }
    gw.with_cost_guard(Arc::clone(&components.cost_guard))
}

/// Configures and registers the gateway web-UI channel.
///
/// When sandbox mode is enabled the function additionally wires a forwarding
/// task that relays job-event SSE frames from the broadcast channel into the
/// gateway's SSE manager.
///
/// Returns a [`GatewaySetup`] containing the computed web-UI URL, the SSE
/// sender, and the routine-engine slot.
pub(crate) async fn setup_gateway_channel(
    config: &Config,
    components: &AppComponents,
    ctx: &mut GatewayContext<'_>,
) -> GatewaySetup {
    let mut gateway_url: Option<String> = None;
    let mut sse_sender: Option<
        tokio::sync::broadcast::Sender<axinite::channels::web::types::SseEvent>,
    > = None;
    let mut routine_engine_slot: Option<axinite::channels::web::server::RoutineEngineSlot> = None;

    if let Some(ref gw_config) = config.channels.gateway {
        let mut gw = configure_gateway_builder(
            GatewayChannel::new_from_context(gw_config.clone(), ctx.env_context.clone()),
            components,
            ctx,
        );
        if config.sandbox.enabled {
            gw = gw.with_prompt_queue(Arc::clone(ctx.prompt_queue));

            if let Some(tx) = ctx.job_event_tx {
                let rx = tx.subscribe();
                let gw_state = Arc::clone(gw.state());
                tokio::spawn(forward_job_events_to_gateway(rx, gw_state));
            }
        }

        let gateway_addr = render_socket_addr(&gw_config.host, gw_config.port);
        gateway_url = Some(format!(
            "http://{}/?token={}",
            gateway_addr,
            gw.auth_token()
        ));

        tracing::debug!("Web UI: http://{gateway_addr}/");

        // IMPORTANT: capture these after all `with_*` calls because `rebuild_state`
        // swaps in a fresh `SseManager`.
        sse_sender = Some(gw.state().sse.sender());
        routine_engine_slot = Some(Arc::clone(&gw.state().routine_engine));

        ctx.channel_names.push("gateway".to_string());
        ctx.channels.add(Box::new(gw)).await;
    }

    GatewaySetup {
        gateway_url,
        sse_sender,
        routine_engine_slot,
    }
}

/// Renders a host and port as a display-safe socket address string.
fn render_socket_addr(host: &str, port: u16) -> String {
    host.parse::<IpAddr>()
        .map(|ip| SocketAddr::new(ip, port).to_string())
        .unwrap_or_else(|_| format!("{host}:{port}"))
}

/// Forwards sandbox job events from the broadcast stream into the gateway SSE
/// manager.
async fn forward_job_events_to_gateway(
    mut rx: tokio::sync::broadcast::Receiver<(uuid::Uuid, axinite::channels::web::types::SseEvent)>,
    gw_state: Arc<axinite::channels::web::server::GatewayState>,
) {
    loop {
        match rx.recv().await {
            Ok((_job_id, event)) => {
                gw_state.sse.broadcast(event);
            }
            Err(tokio::sync::broadcast::error::RecvError::Lagged(skipped)) => {
                tracing::warn!(skipped, "Gateway job-event stream lagged");
            }
            Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
        }
    }
}
