//! Settings API handlers.
//!
//! Most keys are per-user preferences stored in the `settings` table. Keys with
//! the `feature_flag:` prefix are a deployment-scoped exception (RFC 0009):
//! they require an `X-Deployment-Id` header, persist to
//! `feature_flag_overrides` (never the user-scoped `settings` table), and
//! update the in-memory [`FeatureFlagRegistry`] so `GET /api/features` reflects
//! the change without a restart. Reads and deletes of `feature_flag:` keys are
//! rejected here and directed to `GET /api/features`.
//!
//! [`FeatureFlagRegistry`]:
//! crate::channels::web::handlers::feature_registry::FeatureFlagRegistry

use std::sync::Arc;

use axum::{
    Json, Router,
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    routing::{delete, get, post, put},
};

use crate::channels::web::handlers::feature_registry::deployment_id_from_headers;
use crate::channels::web::handlers::features::apply_flag_override;
use crate::channels::web::server::GatewayState;
use crate::channels::web::types::*;
use crate::db::{SettingKey, UserId};

/// Settings key prefix marking a deployment-scoped feature-flag override.
const FEATURE_FLAG_PREFIX: &str = "feature_flag:";

pub fn routes() -> Router<Arc<GatewayState>> {
    Router::new()
        .route("/api/settings", get(settings_list_handler))
        .route("/api/settings/export", get(settings_export_handler))
        .route("/api/settings/import", post(settings_import_handler))
        .route("/api/settings/{key}", get(settings_get_handler))
        .route("/api/settings/{key}", put(settings_set_handler))
        .route("/api/settings/{key}", delete(settings_delete_handler))
}

pub async fn settings_list_handler(
    State(state): State<Arc<GatewayState>>,
) -> Result<Json<SettingsListResponse>, StatusCode> {
    let store = state
        .store
        .as_ref()
        .ok_or(StatusCode::SERVICE_UNAVAILABLE)?;
    let rows = store
        .list_settings(UserId::from(state.user_id.as_str()))
        .await
        .map_err(|e| {
            tracing::error!("Failed to list settings: {}", e);
            StatusCode::INTERNAL_SERVER_ERROR
        })?;

    let settings = rows
        .into_iter()
        .map(|r| SettingResponse {
            key: r.key.as_str().to_string(),
            value: r.value,
            updated_at: r.updated_at.to_rfc3339(),
        })
        .collect();

    Ok(Json(SettingsListResponse { settings }))
}

pub async fn settings_get_handler(
    State(state): State<Arc<GatewayState>>,
    Path(key): Path<String>,
) -> Result<Json<SettingResponse>, StatusCode> {
    // Feature-flag state is deployment-scoped; read it via GET /api/features.
    if key.starts_with(FEATURE_FLAG_PREFIX) {
        return Err(StatusCode::BAD_REQUEST);
    }
    let store = state
        .store
        .as_ref()
        .ok_or(StatusCode::SERVICE_UNAVAILABLE)?;
    let row = store
        .get_setting_full(
            UserId::from(state.user_id.as_str()),
            SettingKey::from(key.as_str()),
        )
        .await
        .map_err(|e| {
            tracing::error!("Failed to get setting '{}': {}", key, e);
            StatusCode::INTERNAL_SERVER_ERROR
        })?
        .ok_or(StatusCode::NOT_FOUND)?;

    Ok(Json(SettingResponse {
        key: row.key.as_str().to_string(),
        value: row.value,
        updated_at: row.updated_at.to_rfc3339(),
    }))
}

pub async fn settings_set_handler(
    State(state): State<Arc<GatewayState>>,
    Path(key): Path<String>,
    headers: HeaderMap,
    Json(body): Json<SettingWriteRequest>,
) -> Result<Response, (StatusCode, String)> {
    // Deployment-scoped feature-flag overrides (RFC 0009) take a separate
    // persistence path and must never touch the user-scoped `settings` table.
    if let Some(flag_name) = key.strip_prefix(FEATURE_FLAG_PREFIX) {
        return set_feature_flag(&state, flag_name, &headers, &body.value).await;
    }

    let store = state
        .store
        .as_ref()
        .ok_or((StatusCode::SERVICE_UNAVAILABLE, "no store".to_string()))?;
    store
        .set_setting(
            UserId::from(state.user_id.as_str()),
            SettingKey::from(key.as_str()),
            &body.value,
        )
        .await
        .map_err(|e| {
            tracing::error!("Failed to set setting '{}': {}", key, e);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                "failed to set setting".to_string(),
            )
        })?;

    Ok(StatusCode::NO_CONTENT.into_response())
}

/// Validate, persist, and cache a deployment-scoped feature-flag override.
///
/// Requires a non-empty `X-Deployment-Id` header, a `[a-z0-9_]+` flag name, and
/// a value that is a JSON boolean or the string `"true"`/`"false"`
/// (case-insensitively). Returns a `SettingResponse`-shaped success body on the
/// happy path.
async fn set_feature_flag(
    state: &GatewayState,
    flag_name: &str,
    headers: &HeaderMap,
    value: &serde_json::Value,
) -> Result<Response, (StatusCode, String)> {
    let deployment_id = deployment_id_from_headers(headers).ok_or((
        StatusCode::BAD_REQUEST,
        "feature_flag writes require a non-empty X-Deployment-Id header".to_string(),
    ))?;

    if !is_valid_flag_name(flag_name) {
        return Err((
            StatusCode::BAD_REQUEST,
            format!("invalid feature flag name '{flag_name}': expected [a-z0-9_]+"),
        ));
    }

    let enabled = coerce_flag_value(value).ok_or((
        StatusCode::BAD_REQUEST,
        "feature flag value must be a JSON boolean or \"true\"/\"false\"".to_string(),
    ))?;

    apply_flag_override(state, &deployment_id, flag_name, enabled)
        .await
        .map_err(|e| {
            tracing::error!(
                deployment_id,
                flag_name,
                "Failed to persist feature flag override: {e}"
            );
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                "failed to persist feature flag override".to_string(),
            )
        })?;

    Ok(Json(SettingResponse {
        key: format!("{FEATURE_FLAG_PREFIX}{flag_name}"),
        value: serde_json::Value::Bool(enabled),
        updated_at: chrono::Utc::now().to_rfc3339(),
    })
    .into_response())
}

/// Flag names are lowercase ASCII letters, digits, and underscores (RFC 0009).
fn is_valid_flag_name(name: &str) -> bool {
    !name.is_empty()
        && name
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_')
}

/// Coerce a settings write value into a boolean: accept a JSON boolean, or the
/// strings `"true"`/`"false"` (case-insensitively). Anything else is rejected.
fn coerce_flag_value(value: &serde_json::Value) -> Option<bool> {
    match value {
        serde_json::Value::Bool(b) => Some(*b),
        serde_json::Value::String(s) if s.eq_ignore_ascii_case("true") => Some(true),
        serde_json::Value::String(s) if s.eq_ignore_ascii_case("false") => Some(false),
        _ => None,
    }
}

pub async fn settings_delete_handler(
    State(state): State<Arc<GatewayState>>,
    Path(key): Path<String>,
) -> Result<StatusCode, StatusCode> {
    // Feature-flag overrides are deployment-scoped; the settings DELETE path
    // only manages user-scoped rows. Deletion is out of scope for RFC 0009's
    // minimal surface, so reject rather than silently no-op.
    if key.starts_with(FEATURE_FLAG_PREFIX) {
        return Err(StatusCode::BAD_REQUEST);
    }
    let store = state
        .store
        .as_ref()
        .ok_or(StatusCode::SERVICE_UNAVAILABLE)?;
    store
        .delete_setting(
            UserId::from(state.user_id.as_str()),
            SettingKey::from(key.as_str()),
        )
        .await
        .map_err(|e| {
            tracing::error!("Failed to delete setting '{}': {}", key, e);
            StatusCode::INTERNAL_SERVER_ERROR
        })?;

    Ok(StatusCode::NO_CONTENT)
}

pub async fn settings_export_handler(
    State(state): State<Arc<GatewayState>>,
) -> Result<Json<SettingsExportResponse>, StatusCode> {
    let store = state
        .store
        .as_ref()
        .ok_or(StatusCode::SERVICE_UNAVAILABLE)?;
    let settings = store
        .get_all_settings(UserId::from(state.user_id.as_str()))
        .await
        .map_err(|e| {
            tracing::error!("Failed to export settings: {}", e);
            StatusCode::INTERNAL_SERVER_ERROR
        })?;

    Ok(Json(SettingsExportResponse { settings }))
}

pub async fn settings_import_handler(
    State(state): State<Arc<GatewayState>>,
    Json(body): Json<SettingsImportRequest>,
) -> Result<StatusCode, StatusCode> {
    let store = state
        .store
        .as_ref()
        .ok_or(StatusCode::SERVICE_UNAVAILABLE)?;
    store
        .set_all_settings(UserId::from(state.user_id.as_str()), &body.settings)
        .await
        .map_err(|e| {
            tracing::error!("Failed to import settings: {}", e);
            StatusCode::INTERNAL_SERVER_ERROR
        })?;

    Ok(StatusCode::NO_CONTENT)
}

#[cfg(test)]
mod tests;
