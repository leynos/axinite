//! Listing handlers for installed extensions and registered tools.

use std::sync::Arc;

use axum::{Json, extract::State, http::StatusCode};

use crate::channels::web::server::GatewayState;
use crate::channels::web::types::{
    ExtensionInfo, ExtensionListResponse, ToolInfo, ToolListResponse,
};

pub async fn extensions_list_handler(
    State(state): State<Arc<GatewayState>>,
) -> Result<Json<ExtensionListResponse>, (StatusCode, String)> {
    let ext_mgr = state.extension_manager.as_ref().ok_or((
        StatusCode::NOT_IMPLEMENTED,
        "Extension manager not available (secrets store required)".to_string(),
    ))?;

    let installed = ext_mgr
        .list(None, false)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    let pairing_store = crate::pairing::PairingStore::new();
    let extensions = installed
        .into_iter()
        .map(|ext| {
            let activation_status = if ext.kind == crate::extensions::ExtensionKind::WasmChannel {
                Some(if ext.activation_error.is_some() {
                    "failed".to_string()
                } else if !ext.authenticated {
                    "installed".to_string()
                } else if ext.active {
                    let has_paired = tokio::task::block_in_place(|| {
                        pairing_store
                            .read_allow_from(&ext.name)
                            .map(|list| !list.is_empty())
                            .unwrap_or(false)
                    });
                    if has_paired {
                        "active".to_string()
                    } else {
                        "pairing".to_string()
                    }
                } else {
                    "configured".to_string()
                })
            } else {
                None
            };
            ExtensionInfo {
                name: ext.name,
                display_name: ext.display_name,
                kind: ext.kind.to_string(),
                description: ext.description,
                url: ext.url,
                authenticated: ext.authenticated,
                active: ext.active,
                tools: ext.tools,
                needs_setup: ext.needs_setup,
                has_auth: ext.has_auth,
                activation_status,
                activation_error: ext.activation_error,
                version: ext.version,
            }
        })
        .collect();

    Ok(Json(ExtensionListResponse { extensions }))
}

pub async fn extensions_tools_handler(
    State(state): State<Arc<GatewayState>>,
) -> Result<Json<ToolListResponse>, (StatusCode, String)> {
    let registry = state.tool_registry.as_ref().ok_or((
        StatusCode::SERVICE_UNAVAILABLE,
        "Tool registry not available".to_string(),
    ))?;

    let definitions = registry.tool_definitions().await;
    let tools = definitions
        .into_iter()
        .map(|td| ToolInfo {
            name: td.name,
            description: td.description,
        })
        .collect();

    Ok(Json(ToolListResponse { tools }))
}
