//! Webhook-server lifecycle helpers used by `tests/webhook_server.rs` and
//! `tests/support_unit_tests/trace_support_module_tests.rs`.

use std::net::SocketAddr;

use anyhow::{Context, Result};
use ironclaw::channels::{WebhookServer, WebhookServerConfig};

use super::webhook_helpers::{health_routes, test_http_client};

/// A started webhook server with a `/health` route and a pre-built client.
pub struct StartedWebhookServer {
    pub server: WebhookServer,
    pub addr: SocketAddr,
    pub client: reqwest::Client,
}

/// Bind an ephemeral listener, build a WebhookServer with a `/health`
/// route, start the server, and return the started server plus a
/// preconfigured client.
pub async fn start_health_server() -> Result<StartedWebhookServer> {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .context("failed to bind webhook health server listener")?;
    let addr = listener
        .local_addr()
        .context("failed to read webhook health server listener address")?;
    let mut server = WebhookServer::new(WebhookServerConfig { addr });
    server.add_routes(health_routes());
    server
        .start_with_listener(listener)
        .await
        .context("failed to start webhook health server")?;
    Ok(StartedWebhookServer {
        server,
        addr,
        client: test_http_client().context("failed to create webhook test HTTP client")?,
    })
}
