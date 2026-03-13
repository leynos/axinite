//! Setup-schema and secret-submission handlers for extensions.

use std::sync::Arc;

use axum::{
    Json,
    extract::{Path, State},
    http::StatusCode,
};

use crate::channels::web::server::GatewayState;
use crate::channels::web::types::{
    ActionResponse, ExtensionSetupRequest, ExtensionSetupResponse, SseEvent,
};

fn internal_error(context: &'static str, error: impl std::fmt::Display) -> (StatusCode, String) {
    tracing::error!(error = %error, "{context}");
    (StatusCode::INTERNAL_SERVER_ERROR, context.to_string())
}

fn logged_failure(
    message: String,
    context: &'static str,
    error: impl std::fmt::Display,
) -> ActionResponse {
    tracing::error!(error = %error, "{context}");
    ActionResponse::fail(message)
}

pub async fn extensions_setup_handler(
    State(state): State<Arc<GatewayState>>,
    Path(name): Path<String>,
) -> Result<Json<ExtensionSetupResponse>, (StatusCode, String)> {
    let ext_mgr = state.extension_manager.as_ref().ok_or((
        StatusCode::NOT_IMPLEMENTED,
        "Extension manager not available (secrets store required)".to_string(),
    ))?;

    let secrets = ext_mgr
        .get_setup_schema(&name)
        .await
        .map_err(|e| internal_error("Failed to load extension setup schema", e))?;

    let installed = ext_mgr
        .list(None, false)
        .await
        .map_err(|e| internal_error("Failed to list installed extensions", e))?;

    let kind = installed
        .into_iter()
        .find(|e| e.name == name)
        .map(|e| e.kind.to_string())
        .unwrap_or_default();

    Ok(Json(ExtensionSetupResponse {
        name,
        kind,
        secrets,
    }))
}

pub async fn extensions_setup_submit_handler(
    State(state): State<Arc<GatewayState>>,
    Path(name): Path<String>,
    Json(req): Json<ExtensionSetupRequest>,
) -> Result<Json<ActionResponse>, (StatusCode, String)> {
    let ext_mgr = state.extension_manager.as_ref().ok_or((
        StatusCode::NOT_IMPLEMENTED,
        "Extension manager not available (secrets store required)".to_string(),
    ))?;

    match ext_mgr.save_setup_secrets(&name, &req.secrets).await {
        Ok(result) => {
            state.sse.broadcast(SseEvent::AuthCompleted {
                extension_name: name.clone(),
                success: true,
                message: result.message.clone(),
            });
            let mut resp = ActionResponse::ok(result.message);
            resp.activated = Some(result.activated);
            resp.auth_url = result.auth_url;
            Ok(Json(resp))
        }
        Err(e) => {
            let message = format!("Failed to save setup secrets for '{}'", name);
            state.sse.broadcast(SseEvent::AuthCompleted {
                extension_name: name.clone(),
                success: false,
                message: message.clone(),
            });
            Ok(Json(logged_failure(
                message,
                "Failed to save extension setup secrets",
                e,
            )))
        }
    }
}
