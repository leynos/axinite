//! Webhook-server lifecycle helpers used only by `tests/webhook_server.rs`.

use std::net::SocketAddr;

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
pub async fn start_health_server()
-> Result<StartedWebhookServer, Box<dyn std::error::Error + Send + Sync>> {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let addr = listener.local_addr()?;
    let mut server = WebhookServer::new(WebhookServerConfig { addr });
    server.add_routes(health_routes());
    server.start_with_listener(listener).await?;
    Ok(StartedWebhookServer {
        server,
        addr,
        client: test_http_client()?,
    })
}
