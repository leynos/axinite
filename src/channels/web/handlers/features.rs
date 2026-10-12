//! Deployment feature flags for the browser UI.
//!
//! Implements the RFC 0009 delivery mechanism: the resolved flag map is exposed
//! at `GET /api/features`. Each flag resolves through the precedence chain
//!
//! 1. `FEATURE_FLAG_<UPPER_SNAKE_NAME>` environment variable (highest),
//! 2. deployment-scoped operator override (from the registry / store),
//! 3. subsystem-availability default (forces a flag off when its backing
//!    subsystem is not wired into `GatewayState`; never enables a flag),
//! 4. compiled default (lowest).
//!
//! For the environment layer, the value `true` (case-insensitively) enables the
//! flag; any other set value disables it; unset falls through to the next
//! layer.
//!
//! Deployment resolution follows the ExecPlan decision: reads use the optional
//! `X-Deployment-Id` header, defaulting to `"default"` when absent so the
//! existing SPA boot fetch keeps working; writes (in the settings handler)
//! require the header. Overrides are cached in the
//! [`FeatureFlagRegistry`](super::feature_registry::FeatureFlagRegistry), which
//! is hydrated only by explicit override commands. Uncached reads load a local
//! snapshot without publishing cache, admission or retry state.

use std::collections::HashMap;
use std::sync::{Arc, OnceLock};

use axum::{
    Json, Router,
    extract::State,
    http::{HeaderMap, HeaderValue, StatusCode},
    response::IntoResponse,
    routing::get,
};

use crate::channels::web::handlers::feature_registry::{
    DEFAULT_DEPLOYMENT_ID, deployment_id_from_headers,
};
use crate::channels::web::server::GatewayState;

use crate::channels::web::handlers::feature_registry::application;
use crate::channels::web::handlers::feature_registry::{SettingsFlagStore, telemetry};
use crate::feature_flags::{
    DeploymentId, FLAG_DEFAULTS, FlagName, FlagOverrideCommand, FlagSnapshot, resolve_flags,
};

/// Routes serving the deployment feature-flag map (`GET /api/features`).
pub fn routes() -> Router<Arc<GatewayState>> {
    Router::new().route("/api/features", get(features_handler))
}

/// Response header carrying the gateway build version so browsers can
/// correlate flag availability with the host build without polluting the flat
/// RFC 0009 body shape.
pub const VERSION_HEADER: &str = "x-axinite-version";

/// Serve resolved flags; persisted-override read failures are explicit 503 errors.
#[tracing::instrument(name = "feature_flags.read_request", skip_all)]
pub async fn features_handler(
    State(state): State<Arc<GatewayState>>,
    headers: HeaderMap,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    let deployment_id = deployment_id_from_headers(&headers)
        .map_err(|error| {
            telemetry::count("read_request", "invalid");
            tracing::warn!(%error, "Rejected feature-flag read: malformed X-Deployment-Id");
            (StatusCode::BAD_REQUEST, error.to_string())
        })?
        .unwrap_or(
            DeploymentId::parse(DEFAULT_DEPLOYMENT_ID)
                .map_err(|error| (StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))?,
        );

    let mut response_headers = HeaderMap::new();
    response_headers.insert(
        VERSION_HEADER,
        HeaderValue::from_static(env!("CARGO_PKG_VERSION")),
    );
    let overrides = query_deployment_snapshot(&state, &deployment_id)
        .await
        .map_err(|error| {
            telemetry::count("read_request", "error");
            tracing::error!(%error, "Failed to load deployment feature flags");
            (
                StatusCode::SERVICE_UNAVAILABLE,
                "failed to load feature flag overrides".to_string(),
            )
        })?;
    let unavailable = unavailable_subsystem_flags(&state).await;

    telemetry::count("read_request", "success");
    Ok((
        response_headers,
        Json(resolve_flags(
            |name| env_flag_overlay().get(name).copied(),
            &overrides,
            &unavailable,
        )),
    ))
}

/// Environment variable consulted for one flag: `FEATURE_FLAG_<UPPER_NAME>`.
fn flag_env_var(name: &str) -> String {
    format!("FEATURE_FLAG_{}", name.to_ascii_uppercase())
}

/// Interpret a raw `FEATURE_FLAG_*` value: `true` (case-insensitively) enables
/// the flag; any other value disables it.
fn parse_env_flag(raw: &str) -> bool {
    raw.eq_ignore_ascii_case("true")
}

/// The environment layer, keyed by flag name and read once per process.
///
/// The `FEATURE_FLAG_<NAME>` variable naming and the string-to-bool reading
/// live here, so [`resolve_flags`] sees only parsed values. A running
/// process's environment cannot be changed from outside, so re-reading it on
/// every `GET /api/features` only repeats work; the overlay is captured on
/// first use and reused for the process lifetime. Only variables that are set
/// for known flags are captured.
pub(super) fn env_flag_overlay() -> &'static HashMap<&'static str, bool> {
    static OVERLAY: OnceLock<HashMap<&'static str, bool>> = OnceLock::new();
    OVERLAY.get_or_init(|| {
        FLAG_DEFAULTS
            .iter()
            .filter_map(|(name, _)| {
                std::env::var(flag_env_var(name))
                    .ok()
                    .map(|raw| (*name, parse_env_flag(&raw)))
            })
            .collect()
    })
}

/// Flags whose backing subsystem is absent from `GatewayState`, per the
/// registry's own `backendContract` metadata (for example `route_jobs` is
/// "hide when jobs runtime is absent").
///
/// The subsystem layer only ever *disables*: presence of a subsystem falls
/// through to the compiled default rather than enabling a flag early.
async fn unavailable_subsystem_flags(state: &GatewayState) -> Vec<&'static str> {
    let scheduler_present = match state.scheduler.as_ref() {
        Some(slot) => slot.read().await.is_some(),
        None => false,
    };
    crate::feature_flags::availability::SubsystemAvailability {
        jobs: state.job_manager.is_some() || scheduler_present,
        routines: state.routine_engine.read().await.is_some(),
        extensions: state.extension_manager.is_some(),
        skills: state.skill_registry.is_some(),
        logs: state.log_broadcaster.is_some(),
    }
    .unavailable_flags()
}

/// Return a detached response snapshot; observations do not publish registry state.
async fn query_deployment_snapshot(
    state: &GatewayState,
    deployment: &DeploymentId,
) -> Result<FlagSnapshot, application::FlagStoreError> {
    let resources = state.feature_flags.read().await.resources();
    telemetry::cache_resources(resources);
    let cached = state.feature_flags.read().await.snapshot(deployment);
    if let Some(snapshot) = cached {
        telemetry::count("cache", "hit");
        return Ok(snapshot);
    }
    let Some(store) = state.store.as_ref() else {
        telemetry::count("read_snapshot", "no_store_defaults");
        return Ok(FlagSnapshot::default());
    };
    let adapter = SettingsFlagStore {
        store: store.as_ref(),
    };
    application::query(&state.feature_flags, &adapter, deployment).await
}

/// Persist through the gateway command; failed writes never publish their new value.
pub(crate) async fn apply_flag_override(
    state: &GatewayState,
    deployment: &DeploymentId,
    flag: &FlagName,
    enabled: bool,
) -> Result<(), crate::error::DatabaseError> {
    let store = state
        .store
        .as_ref()
        .ok_or_else(|| crate::error::DatabaseError::Query("no store configured".to_string()))?;
    let adapter = SettingsFlagStore {
        store: store.as_ref(),
    };
    application::set_override(
        &state.feature_flags,
        &adapter,
        FlagOverrideCommand {
            deployment: deployment.clone(),
            flag: flag.clone(),
            enabled,
        },
    )
    .await
    .map_err(|error| crate::error::DatabaseError::Query(error.to_string()))
}

#[cfg(test)]
mod tests;
