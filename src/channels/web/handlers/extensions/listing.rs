//! Listing handlers for installed extensions and registered tools.

use std::collections::HashSet;
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

    let pairing_tasks = installed
        .iter()
        .filter(|ext| {
            ext.kind == crate::extensions::ExtensionKind::WasmChannel
                && ext.authenticated
                && ext.active
        })
        .map(|ext| {
            let name = ext.name.clone();
            let handle = tokio::task::spawn_blocking({
                let name = name.clone();
                move || {
                    crate::pairing::PairingStore::new()
                        .read_allow_from(&name)
                        .map(|list| !list.is_empty())
                        .unwrap_or(false)
                }
            });
            (name, handle)
        })
        .collect::<Vec<_>>();

    let mut paired_names = HashSet::new();
    for (name, handle) in pairing_tasks {
        match handle.await {
            Ok(true) => {
                paired_names.insert(name);
            }
            Ok(false) => {}
            Err(error) => {
                tracing::error!(
                    extension_name = %name,
                    error = %error,
                    "Failed to join pairing lookup task"
                );
            }
        }
    }

    let extensions = installed
        .into_iter()
        .map(|ext| {
            let activation_status = if ext.kind == crate::extensions::ExtensionKind::WasmChannel {
                Some(if ext.activation_error.is_some() {
                    "failed".to_string()
                } else if !ext.authenticated {
                    "installed".to_string()
                } else if ext.active {
                    if paired_names.contains(&ext.name) {
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
