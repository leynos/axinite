//! Install, activate, and remove handlers for extensions.

use std::sync::Arc;

use axum::{
    Json,
    extract::{Path, State},
    http::StatusCode,
};

use crate::channels::web::server::GatewayState;
use crate::channels::web::types::{ActionResponse, InstallExtensionRequest};

use super::common::{activation_required_response, maybe_extension_auth_url};

pub async fn extensions_install_handler(
    State(state): State<Arc<GatewayState>>,
    Json(req): Json<InstallExtensionRequest>,
) -> Result<Json<ActionResponse>, (StatusCode, String)> {
    let Some(ext_mgr) = state.extension_manager.as_ref() else {
        if let Some(entry) = state.registry_entries.iter().find(|e| e.name == req.name) {
            let msg = match &entry.source {
                crate::extensions::ExtensionSource::WasmBuildable { .. } => {
                    format!(
                        "'{}' requires building from source. \
                         Run `ironclaw registry install {}` from the CLI.",
                        req.name, req.name
                    )
                }
                _ => format!(
                    "Extension manager not available (secrets store required). \
                     Configure DATABASE_URL or a secrets backend to enable installation of '{}'.",
                    req.name
                ),
            };
            return Ok(Json(ActionResponse::fail(msg)));
        }
        return Ok(Json(ActionResponse::fail(
            "Extension manager not available (secrets store required)".to_string(),
        )));
    };

    let kind_hint = req.kind.as_deref().and_then(|k| match k {
        "mcp_server" => Some(crate::extensions::ExtensionKind::McpServer),
        "wasm_tool" => Some(crate::extensions::ExtensionKind::WasmTool),
        "wasm_channel" => Some(crate::extensions::ExtensionKind::WasmChannel),
        _ => None,
    });

    match ext_mgr
        .install(&req.name, req.url.as_deref(), kind_hint)
        .await
    {
        Ok(result) => {
            let mut resp = ActionResponse::ok(result.message);

            if result.kind == crate::extensions::ExtensionKind::WasmTool {
                match ext_mgr.activate(&req.name).await {
                    Ok(_) => {}
                    Err(crate::extensions::ExtensionError::AuthRequired) => {
                        return Ok(Json(
                            activation_required_response(
                                ext_mgr,
                                &req.name,
                                format!(
                                    "Installed '{}' but activation requires authentication.",
                                    req.name
                                ),
                                format!("Installed '{}' but authentication setup failed", req.name),
                            )
                            .await,
                        ));
                    }
                    Err(e) => {
                        return Ok(Json(ActionResponse::fail(format!(
                            "Installed '{}' but activation failed: {}",
                            req.name, e
                        ))));
                    }
                }

                resp.auth_url = maybe_extension_auth_url(ext_mgr, &req.name).await;
            }

            Ok(Json(resp))
        }
        Err(e) => Ok(Json(ActionResponse::fail(e.to_string()))),
    }
}

pub async fn extensions_activate_handler(
    State(state): State<Arc<GatewayState>>,
    Path(name): Path<String>,
) -> Result<Json<ActionResponse>, (StatusCode, String)> {
    let ext_mgr = state.extension_manager.as_ref().ok_or((
        StatusCode::NOT_IMPLEMENTED,
        "Extension manager not available (secrets store required)".to_string(),
    ))?;

    match ext_mgr.activate(&name).await {
        Ok(result) => {
            let mut resp = ActionResponse::ok(result.message);
            resp.auth_url = maybe_extension_auth_url(ext_mgr, &name).await;
            Ok(Json(resp))
        }
        Err(activate_err) => {
            if !matches!(
                &activate_err,
                crate::extensions::ExtensionError::AuthRequired
            ) {
                return Ok(Json(ActionResponse::fail(activate_err.to_string())));
            }

            if let Ok(auth_result) = ext_mgr.auth(&name, None).await
                && auth_result.is_authenticated()
            {
                return match ext_mgr.activate(&name).await {
                    Ok(result) => Ok(Json(ActionResponse::ok(result.message))),
                    Err(e) => Ok(Json(ActionResponse::fail(e.to_string()))),
                };
            }

            Ok(Json(
                activation_required_response(
                    ext_mgr,
                    &name,
                    format!("'{}' requires authentication.", name),
                    "Authentication failed".to_string(),
                )
                .await,
            ))
        }
    }
}

pub async fn extensions_remove_handler(
    State(state): State<Arc<GatewayState>>,
    Path(name): Path<String>,
) -> Result<Json<ActionResponse>, (StatusCode, String)> {
    let ext_mgr = state.extension_manager.as_ref().ok_or((
        StatusCode::NOT_IMPLEMENTED,
        "Extension manager not available (secrets store required)".to_string(),
    ))?;

    match ext_mgr.remove(&name).await {
        Ok(message) => Ok(Json(ActionResponse::ok(message))),
        Err(e) => Ok(Json(ActionResponse::fail(e.to_string()))),
    }
}
