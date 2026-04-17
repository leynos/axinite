//! Channel, gateway, and WASM runtime wiring for process startup.

use std::{
    net::{IpAddr, SocketAddr},
    sync::Arc,
};

use ironclaw::{
    app::AppComponents,
    channels::{
        ChannelManager, GatewayChannel, HttpChannel, ReplChannel, SignalChannel, WebhookServer,
        WebhookServerConfig, web::log_layer::LogBroadcaster,
    },
    cli::Cli,
    config::Config,
};

use crate::startup::wasm::{WasmChannelRuntimeState, WasmChannelsInit, init_wasm_channels};

/// Aggregated results returned after all process-startup channel wiring has
/// completed.
pub(crate) struct ChannelSetup {
    /// Optional started webhook server; `None` when no channel registered HTTP
    /// routes.
    pub(crate) webhook_server: Option<Arc<tokio::sync::Mutex<WebhookServer>>>,
    /// Names of every enabled channel, collected during setup.
    pub(crate) channel_names: Vec<String>,
    /// Names of successfully loaded WASM channels.
    pub(crate) loaded_wasm_channel_names: Vec<String>,
    /// Optional WASM channel runtime state to be wired in later; `None` when
    /// WASM is disabled.
    pub(crate) wasm_channel_runtime_state: Option<WasmChannelRuntimeState>,
    /// (Unix only) Optional shared HTTP channel state for secret-updater
    /// wiring.
    #[cfg(unix)]
    pub(crate) http_channel_state: Option<Arc<ironclaw::channels::HttpChannelState>>,
}

struct HttpChannelResult {
    webhook_server_addr: Option<std::net::SocketAddr>,
    #[cfg(unix)]
    http_channel_state: Option<Arc<ironclaw::channels::HttpChannelState>>,
}

/// Registration sinks shared across channel-setup helpers.
pub(crate) struct ChannelRegistrar<'a> {
    /// Shared channel manager used to register new channels.
    pub(crate) channels: &'a ChannelManager,
    /// Accumulates the name of every registered channel.
    pub(crate) channel_names: &'a mut Vec<String>,
    /// Accumulates Axum routers contributed by channels with webhook endpoints.
    pub(crate) webhook_routes: &'a mut Vec<axum::Router>,
}

/// Runtime-service dependencies required to configure the gateway channel.
pub(crate) struct GatewayContext<'a> {
    /// Container job manager exposed to the gateway for sandbox operations.
    pub(crate) container_job_manager: &'a Option<Arc<ironclaw::orchestrator::ContainerJobManager>>,
    /// Session manager used by gateway sessions.
    pub(crate) session_manager: &'a Arc<ironclaw::agent::SessionManager>,
    /// Log broadcaster backing live gateway log streaming.
    pub(crate) log_broadcaster: &'a Arc<LogBroadcaster>,
    /// Shared log-level handle used by the gateway UI.
    pub(crate) log_level_handle: &'a Arc<ironclaw::channels::web::log_layer::LogLevelHandle>,
    /// Prompt queue shared with sandbox-backed job interactions.
    pub(crate) prompt_queue: &'a Arc<
        tokio::sync::Mutex<
            std::collections::HashMap<
                uuid::Uuid,
                std::collections::VecDeque<ironclaw::orchestrator::api::PendingPrompt>,
            >,
        >,
    >,
    /// Scheduler slot injected into the gateway after startup.
    pub(crate) scheduler_slot: &'a ironclaw::tools::builtin::SchedulerSlot,
    /// Broadcast sender for relaying job events into the gateway.
    pub(crate) job_event_tx: &'a Option<
        tokio::sync::broadcast::Sender<(uuid::Uuid, ironclaw::channels::web::types::SseEvent)>,
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
        Option<tokio::sync::broadcast::Sender<ironclaw::channels::web::types::SseEvent>>,
    /// Slot for injecting a routine engine into the gateway after startup.
    pub(crate) routine_engine_slot: Option<ironclaw::channels::web::server::RoutineEngineSlot>,
}

/// Registers the interactive REPL channel when CLI mode is enabled.
async fn setup_repl_channel(cli: &Cli, config: &Config, reg: &mut ChannelRegistrar<'_>) {
    let repl_channel = if let Some(ref msg) = cli.message {
        Some(ReplChannel::with_message(msg.clone()))
    } else if config.channels.cli.enabled {
        let repl = ReplChannel::new();
        repl.suppress_banner();
        Some(repl)
    } else {
        None
    };
    let Some(repl) = repl_channel else { return };
    reg.channels.add(Box::new(repl)).await;
    if cli.message.is_some() {
        tracing::debug!("Single message mode");
    } else {
        reg.channel_names.push("repl".to_string());
        tracing::debug!("REPL mode enabled");
    }
}

/// Registers the Signal channel and validates its allowlist configuration.
async fn setup_signal_channel(
    cli: &Cli,
    config: &Config,
    reg: &mut ChannelRegistrar<'_>,
) -> anyhow::Result<()> {
    if cli.cli_only {
        return Ok(());
    }
    let Some(ref signal_config) = config.channels.signal else {
        return Ok(());
    };
    let signal_channel = SignalChannel::new(signal_config.clone())?;
    reg.channel_names.push("signal".to_string());
    reg.channels.add(Box::new(signal_channel)).await;
    let safe_url = SignalChannel::redact_url(&signal_config.http_url);
    tracing::debug!(url = %safe_url, "Signal channel enabled");
    if signal_config.allow_from.is_empty() {
        tracing::warn!("Signal channel has empty allow_from list - ALL messages will be DENIED.");
    }
    Ok(())
}

/// Registers the HTTP channel and returns any shared webhook server state it
/// exposes.
async fn setup_http_channel(
    cli: &Cli,
    config: &Config,
    reg: &mut ChannelRegistrar<'_>,
) -> anyhow::Result<HttpChannelResult> {
    let none_result = HttpChannelResult {
        webhook_server_addr: None,
        #[cfg(unix)]
        http_channel_state: None,
    };
    if cli.cli_only {
        return Ok(none_result);
    }
    let Some(ref http_config) = config.channels.http else {
        return Ok(none_result);
    };
    let http_channel = HttpChannel::new(http_config.clone());
    #[cfg(unix)]
    let http_channel_state = Some(http_channel.shared_state());
    reg.webhook_routes.push(http_channel.routes());
    let (host, port) = http_channel.addr();
    let webhook_server_addr = Some(parse_socket_addr(host, port, "HTTP")?);
    reg.channel_names.push("http".to_string());
    reg.channels.add(Box::new(http_channel)).await;
    tracing::debug!(
        "HTTP channel enabled on {}:{}",
        http_config.host,
        http_config.port
    );
    Ok(HttpChannelResult {
        webhook_server_addr,
        #[cfg(unix)]
        http_channel_state,
    })
}

/// Starts the shared webhook server when any channel has registered routes.
async fn build_webhook_server(
    addr: Option<std::net::SocketAddr>,
    http_bind_was_explicit: bool,
    webhook_routes: Vec<axum::Router>,
) -> anyhow::Result<Option<Arc<tokio::sync::Mutex<WebhookServer>>>> {
    if webhook_routes.is_empty() {
        return Ok(None);
    }
    let addr = addr.unwrap_or_else(|| std::net::SocketAddr::from(([127, 0, 0, 1], 8080)));
    if addr.ip().is_unspecified() {
        anyhow::ensure!(
            http_bind_was_explicit,
            "Refusing to bind webhook server to {addr}. Configure an explicit HTTP bind address \
             or use a loopback-only address such as 127.0.0.1:8080."
        );
    }
    let mut server = WebhookServer::new(WebhookServerConfig { addr });
    for routes in webhook_routes {
        server.add_routes(routes);
    }
    server.start().await?;
    Ok(Some(Arc::new(tokio::sync::Mutex::new(server))))
}

/// Initializes all configured channels (REPL, signal, HTTP, and WASM) and
/// starts the webhook server when any channel has registered HTTP routes.
///
/// Returns a [`ChannelSetup`] bundle that carries the started webhook server,
/// the list of enabled channel names, and the optional WASM runtime state.
pub(crate) async fn setup_channels(
    cli: &Cli,
    config: &Config,
    components: &AppComponents,
    channels: &ChannelManager,
) -> anyhow::Result<ChannelSetup> {
    let mut channel_names: Vec<String> = Vec::new();
    let mut webhook_routes: Vec<axum::Router> = Vec::new();
    let (loaded_wasm_channel_names, wasm_channel_runtime_state, http) = {
        let mut reg = ChannelRegistrar {
            channels,
            channel_names: &mut channel_names,
            webhook_routes: &mut webhook_routes,
        };

        setup_repl_channel(cli, config, &mut reg).await;

        let WasmChannelsInit {
            loaded_wasm_channel_names,
            runtime_state: wasm_channel_runtime_state,
        } = init_wasm_channels(config, components, &mut reg).await;

        setup_signal_channel(cli, config, &mut reg).await?;

        let http = setup_http_channel(cli, config, &mut reg).await?;

        (loaded_wasm_channel_names, wasm_channel_runtime_state, http)
    };

    let webhook_server = build_webhook_server(
        http.webhook_server_addr,
        config.channels.http.is_some(),
        webhook_routes,
    )
    .await?;

    Ok(ChannelSetup {
        webhook_server,
        channel_names,
        loaded_wasm_channel_names,
        wasm_channel_runtime_state,
        #[cfg(unix)]
        http_channel_state: http.http_channel_state,
    })
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
        tokio::sync::broadcast::Sender<ironclaw::channels::web::types::SseEvent>,
    > = None;
    let mut routine_engine_slot: Option<ironclaw::channels::web::server::RoutineEngineSlot> = None;

    if let Some(ref gw_config) = config.channels.gateway {
        let mut gw =
            configure_gateway_builder(GatewayChannel::new(gw_config.clone()), components, ctx);
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

/// Parses a validated socket address from a host and port pair.
fn parse_socket_addr(host: &str, port: u16, channel_name: &str) -> anyhow::Result<SocketAddr> {
    let host: IpAddr = host
        .parse()
        .map_err(|e| anyhow::anyhow!("Invalid {channel_name} host '{host}': {e}"))?;
    Ok(SocketAddr::new(host, port))
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
    mut rx: tokio::sync::broadcast::Receiver<(
        uuid::Uuid,
        ironclaw::channels::web::types::SseEvent,
    )>,
    gw_state: Arc<ironclaw::channels::web::server::GatewayState>,
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

/// Spawns a Tokio task that listens for `SIGHUP` and triggers a hot-reload.
///
/// The task exits cleanly when the shutdown broadcast fires. Only compiled on
/// Unix targets.
#[cfg(unix)]
pub(crate) fn spawn_sighup_handler(
    reload_manager: Arc<ironclaw::reload::HotReloadManager>,
    shutdown_tx: &tokio::sync::broadcast::Sender<()>,
) {
    let mut shutdown_rx = shutdown_tx.subscribe();
    tokio::spawn(async move {
        use tokio::signal::unix::{SignalKind, signal};
        let mut sighup = match signal(SignalKind::hangup()) {
            Ok(s) => s,
            Err(e) => {
                tracing::warn!("Failed to register SIGHUP handler: {e}");
                return;
            }
        };
        loop {
            tokio::select! {
                _ = shutdown_rx.recv() => {
                    tracing::debug!("SIGHUP handler shutting down");
                    break;
                }
                _ = sighup.recv() => {}
            }
            tracing::info!("SIGHUP received — reloading HTTP webhook config");
            if let Err(e) = reload_manager.perform_reload().await {
                tracing::error!("Hot-reload failed: {e}");
            }
        }
    });
}
