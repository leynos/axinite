//! Setup-schema and setup-submission handlers for extensions.

use std::sync::Arc;

use axum::{
    Json,
    extract::{Path, State},
    http::StatusCode,
};

use crate::channels::web::handlers::common::internal_error;
use crate::channels::web::server::GatewayState;
use crate::channels::web::types::{
    EndpointFieldInfo, ExtensionSetupRequest, ExtensionSetupResponse, ExtensionSetupSubmitResponse,
    SecretFieldInfo, SetupBlockerInfo, SetupOutcomeInfo, SseEvent,
};
use crate::extensions::setup::{CredentialName, EndpointName, SetupField, SetupOperation};
use crate::extensions::{ExtensionError, ExtensionManager};

pub async fn extensions_setup_handler(
    State(state): State<Arc<GatewayState>>,
    Path(name): Path<String>,
) -> Result<Json<ExtensionSetupResponse>, (StatusCode, String)> {
    let ext_mgr = state.extension_manager.as_ref().ok_or((
        StatusCode::NOT_IMPLEMENTED,
        "Extension manager not available (secrets store required)".to_string(),
    ))?;

    let snapshot = ext_mgr
        .setup_snapshot(&name)
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
        secrets: secret_fields(&snapshot),
        endpoints: endpoint_fields(&snapshot),
        ready: snapshot.ready(),
        blockers: blocker_fields(&snapshot),
    }))
}

pub async fn extensions_setup_submit_handler(
    State(state): State<Arc<GatewayState>>,
    Path(name): Path<String>,
    Json(req): Json<ExtensionSetupRequest>,
) -> Result<Json<ExtensionSetupSubmitResponse>, (StatusCode, String)> {
    let ext_mgr = state.extension_manager.as_ref().ok_or((
        StatusCode::NOT_IMPLEMENTED,
        "Extension manager not available (secrets store required)".to_string(),
    ))?;

    let operations = match build_operations(&req) {
        Ok(operations) => operations,
        Err(category) => return Ok(Json(rejected_without_snapshot(category))),
    };

    match ext_mgr.apply_setup(&name, operations).await {
        Ok(application) => {
            broadcast(&state, &name, true, application.message());
            Ok(Json(ExtensionSetupSubmitResponse {
                success: !application
                    .outcomes()
                    .iter()
                    .any(|report| report.outcome().is_ambiguous()),
                message: application.message().to_string(),
                activated: application.activated(),
                auth_url: application.auth_url().map(String::from),
                category: None,
                ready: application.snapshot().ready(),
                outcomes: application
                    .outcomes()
                    .iter()
                    .map(|report| SetupOutcomeInfo {
                        field: report.field().as_str().to_string(),
                        kind: report.field().kind().to_string(),
                        outcome: report.outcome().as_str().to_string(),
                    })
                    .collect(),
                endpoints: endpoint_fields(application.snapshot()),
                blockers: blocker_fields(application.snapshot()),
            }))
        }
        Err(error) => Ok(Json(rejection(ext_mgr, &name, &error).await)),
    }
}

/// Translate the request body into typed operations, rejecting bad key names.
///
/// The error is the stable, value-free category; the caller builds the
/// response so this function stays cheap to call.
fn build_operations(req: &ExtensionSetupRequest) -> Result<Vec<SetupOperation>, &'static str> {
    let mut operations = Vec::new();
    for (name, value) in sorted_pairs(&req.secrets) {
        let name = CredentialName::parse(name).map_err(|error| error.category())?;
        operations.push(SetupOperation::Secret {
            name,
            value: value.to_string(),
        });
    }
    for (name, url) in sorted_pairs(&req.endpoints.set) {
        let name = EndpointName::parse(name).map_err(|error| error.category())?;
        operations.push(SetupOperation::EndpointSet {
            name,
            url: url.to_string(),
        });
    }
    for name in &req.endpoints.clear {
        let name = EndpointName::parse(name.clone()).map_err(|error| error.category())?;
        operations.push(SetupOperation::EndpointClear { name });
    }
    Ok(operations)
}

/// Sort request entries so the response order is deterministic.
fn sorted_pairs(map: &std::collections::HashMap<String, String>) -> Vec<(&String, &String)> {
    let mut pairs: Vec<_> = map.iter().collect();
    pairs.sort_by(|left, right| left.0.cmp(right.0));
    pairs
}

fn rejected_without_snapshot(category: &'static str) -> ExtensionSetupSubmitResponse {
    ExtensionSetupSubmitResponse {
        success: false,
        message: format!("Setup rejected: {category}"),
        activated: false,
        auth_url: None,
        category: Some(category.to_string()),
        ready: false,
        outcomes: Vec::new(),
        endpoints: Vec::new(),
        blockers: Vec::new(),
    }
}

/// Build the response for a rejected or failed submission.
async fn rejection(
    ext_mgr: &Arc<ExtensionManager>,
    name: &str,
    error: &ExtensionError,
) -> ExtensionSetupSubmitResponse {
    let category = match error {
        ExtensionError::SetupRejected { category, .. } => Some((*category).to_string()),
        _ => None,
    };
    let message = match error {
        ExtensionError::SetupRejected { category, .. } => {
            format!("Setup rejected: {category}")
        }
        other => {
            tracing::warn!(
                extension = name,
                error = %other,
                "Extension setup submission failed"
            );
            format!("Failed to save setup for '{name}'")
        }
    };
    let (ready, endpoints, blockers) = match ext_mgr.setup_snapshot(name).await {
        Ok(snapshot) => (
            snapshot.ready(),
            endpoint_fields(&snapshot),
            blocker_fields(&snapshot),
        ),
        Err(_) => (false, Vec::new(), Vec::new()),
    };
    ExtensionSetupSubmitResponse {
        success: false,
        message,
        activated: false,
        auth_url: None,
        category,
        ready,
        outcomes: Vec::new(),
        endpoints,
        blockers,
    }
}

fn broadcast(state: &GatewayState, name: &str, success: bool, message: &str) {
    state.sse.broadcast(SseEvent::AuthCompleted {
        extension_name: name.to_string(),
        success,
        message: message.to_string(),
    });
}

/// Secret fields in declaration order.
pub(super) fn secret_fields(snapshot: &crate::extensions::SetupSnapshot) -> Vec<SecretFieldInfo> {
    snapshot
        .declarations()
        .iter()
        .filter_map(|field| match field {
            SetupField::Secret(secret) => Some(SecretFieldInfo {
                name: secret.name.as_str().to_string(),
                prompt: secret.prompt.clone(),
                optional: secret.optional,
                provided: snapshot.secret_provided(&secret.name) == Some(true),
                auto_generate: secret.auto_generate,
            }),
            SetupField::Endpoint(_) => None,
        })
        .collect()
}

/// Endpoint fields in declaration order, with their stored state.
pub(super) fn endpoint_fields(
    snapshot: &crate::extensions::SetupSnapshot,
) -> Vec<EndpointFieldInfo> {
    snapshot
        .declarations()
        .iter()
        .filter_map(|field| match field {
            SetupField::Endpoint(endpoint) => {
                let state = snapshot
                    .endpoint_state(&endpoint.name)
                    .map_or("missing", |state| match state {
                        crate::extensions::setup::EndpointState::Missing => "missing",
                        crate::extensions::setup::EndpointState::Configured => "configured",
                        crate::extensions::setup::EndpointState::Stale => "stale",
                        crate::extensions::setup::EndpointState::Corrupt => "corrupt",
                    });
                Some(EndpointFieldInfo {
                    name: endpoint.name.as_str().to_string(),
                    kind: endpoint.kind.as_str().to_string(),
                    prompt: endpoint.prompt.clone(),
                    optional: endpoint.optional,
                    state: state.to_string(),
                })
            }
            SetupField::Secret(_) => None,
        })
        .collect()
}

/// Readiness blockers in declaration order.
pub(super) fn blocker_fields(snapshot: &crate::extensions::SetupSnapshot) -> Vec<SetupBlockerInfo> {
    snapshot
        .blockers()
        .into_iter()
        .map(|blocker| SetupBlockerInfo {
            field: blocker.field().as_str().to_string(),
            kind: blocker.field().kind().to_string(),
            reason: blocker.reason().as_str().to_string(),
        })
        .collect()
}
