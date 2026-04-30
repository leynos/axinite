//! Shared webhook helper functions used across webhook-focused harnesses.

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
