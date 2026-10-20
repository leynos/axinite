//! Channel and WASM runtime wiring for process startup.

pub(crate) mod gateway;
pub(crate) use gateway::{GatewayContext, GatewaySetup, setup_gateway_channel};

use std::{
    net::{IpAddr, SocketAddr},
    sync::Arc,
};

use axinite::{
    app::AppComponents,
    channels::{
        ChannelManager, HttpChannel, ReplChannel, SignalChannel, WebhookServer, WebhookServerConfig,
    },
    cli::Cli,
    config::Config,
};

use crate::startup::wasm::{WasmChannelRuntimeState, WasmChannelsInit, init_wasm_channels};

/// Aggregated results returned after process-startup channel wiring completes.
pub(crate) struct ChannelSetup {
    /// Optional started webhook server; `None` when no channel registered HTTP routes.
    pub(crate) webhook_server: Option<Arc<tokio::sync::Mutex<WebhookServer>>>,
    /// Names of every enabled channel, collected during setup.
    pub(crate) channel_names: Vec<String>,
    /// Names of successfully loaded WASM channels.
    pub(crate) loaded_wasm_channel_names: Vec<String>,
    /// Optional WASM channel runtime state to be wired in later; `None` when WASM is disabled.
    pub(crate) wasm_channel_runtime_state: Option<WasmChannelRuntimeState>,
    /// (Unix only) Optional shared HTTP channel state for secret-updater wiring.
    #[cfg(unix)]
    pub(crate) http_channel_state: Option<Arc<axinite::channels::HttpChannelState>>,
}

struct HttpChannelResult {
    webhook_server_addr: Option<std::net::SocketAddr>,
    #[cfg(unix)]
    http_channel_state: Option<Arc<axinite::channels::HttpChannelState>>,
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

/// Initializes all configured channels (REPL, signal, HTTP, and WASM) and starts
/// the webhook server when any channel has registered HTTP routes.
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
    let (loaded_wasm_channel_names, wasm_channel_runtime_state, http, webhook_routes) = {
        let mut reg = ChannelRegistrar {
            channels,
            channel_names: &mut channel_names,
            webhook_routes: &mut webhook_routes,
        };

        setup_repl_channel(cli, config, &mut reg).await;

        let WasmChannelsInit {
            loaded_wasm_channel_names,
            runtime_state: wasm_channel_runtime_state,
        } = if cli.cli_only {
            WasmChannelsInit {
                loaded_wasm_channel_names: vec![],
                runtime_state: None,
            }
        } else {
            init_wasm_channels(config, components, &mut reg).await
        };

        setup_signal_channel(cli, config, &mut reg).await?;

        let http = setup_http_channel(cli, config, &mut reg).await?;

        (
            loaded_wasm_channel_names,
            wasm_channel_runtime_state,
            http,
            webhook_routes,
        )
    };

    let webhook_server = if cli.cli_only || webhook_routes.is_empty() {
        None
    } else {
        build_webhook_server(
            http.webhook_server_addr,
            config.channels.http.is_some(),
            webhook_routes,
        )
        .await?
    };

    Ok(ChannelSetup {
        webhook_server,
        channel_names,
        loaded_wasm_channel_names,
        wasm_channel_runtime_state,
        #[cfg(unix)]
        http_channel_state: http.http_channel_state,
    })
}

/// Parses a validated socket address from a host and port pair.
fn parse_socket_addr(host: &str, port: u16, channel_name: &str) -> anyhow::Result<SocketAddr> {
    let host: IpAddr = host
        .parse()
        .map_err(|e| anyhow::anyhow!("Invalid {channel_name} host '{host}': {e}"))?;
    Ok(SocketAddr::new(host, port))
}

#[cfg(test)]
mod tests;
