//! Shared HTTP-route and client helpers for webhook-server integration tests.
//!
//! These helpers are used by the webhook-server harness. Server-lifecycle
//! helpers live in `webhook_server_helpers.rs` so infrastructure-only binaries
//! do not compile unused server wrappers.
//!
//! The infrastructure harness wires `tests/support/webhook_common.rs` into its
//! own `webhook_helpers` module through `#[path]` in
//! `tests/support/infrastructure.rs`.

use std::time::Duration;

use axum::Json;
use axum::Router;
use axum::routing::get;
use serde_json::json;

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
