//! Shared HTTP server state, request types, and health endpoint for the
//! WASM channel webhook router.

use std::sync::Arc;

use axum::{Json, extract::State, response::IntoResponse};
use serde::{Deserialize, Serialize};

use super::WasmChannelRouter;

/// Shared state for the HTTP server.
#[allow(dead_code)]
#[derive(Clone)]
pub struct RouterState {
    pub(super) router: Arc<WasmChannelRouter>,
    pub(super) extension_manager: Option<Arc<crate::extensions::ExtensionManager>>,
}

impl RouterState {
    pub fn new(router: Arc<WasmChannelRouter>) -> Self {
        Self {
            router,
            extension_manager: None,
        }
    }

    pub fn with_extension_manager(
        mut self,
        manager: Arc<crate::extensions::ExtensionManager>,
    ) -> Self {
        self.extension_manager = Some(manager);
        self
    }
}

/// Webhook request body for WASM channels.
#[allow(dead_code)]
#[derive(Debug, Deserialize)]
pub struct WasmWebhookRequest {
    /// Optional secret for authentication.
    #[serde(default)]
    pub secret: Option<String>,
}

/// Health response.
#[allow(dead_code)]
#[derive(Debug, Serialize)]
struct HealthResponse {
    status: String,
    channels: Vec<String>,
}

/// Handler for health check endpoint.
#[allow(dead_code)]
pub(super) async fn health_handler(State(state): State<RouterState>) -> impl IntoResponse {
    let channels = state.router.list_channels().await;
    Json(HealthResponse {
        status: "healthy".to_string(),
        channels,
    })
}
