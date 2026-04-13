//! Shared helpers for WebhookServer integration tests.
//!
//! Provides reusable server setup and client construction so that
//! `tests/webhook_server.rs` and `tests/infrastructure/sighup_reload.rs`
//! share the same configuration.

use std::net::SocketAddr;
use std::time::Duration;

use axum::Json;
use axum::Router;
use axum::routing::get;
use serde_json::json;

use ironclaw::channels::{WebhookServer, WebhookServerConfig};

/// A started webhook server with a `/health` route and a pre-built client.
pub struct StartedWebhookServer {
    pub server: WebhookServer,
    pub addr: SocketAddr,
    pub client: reqwest::Client,
}

/// Return the standard `/health` check route used by webhook tests.
pub fn health_routes() -> Router {
    Router::new().route("/health", get(|| async { Json(json!({"status": "ok"})) }))
}

/// Build a reqwest client with the standard 2-second test timeout.
pub fn test_http_client() -> Result<reqwest::Client, reqwest::Error> {
    reqwest::Client::builder()
        .timeout(Duration::from_secs(2))
        .build()
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
