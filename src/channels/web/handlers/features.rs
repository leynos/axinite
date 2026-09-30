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
//! is hydrated lazily from the store on the first read for a deployment.

use std::collections::{BTreeMap, HashMap};
use std::sync::{Arc, OnceLock};

use axum::{
    Json, Router,
    extract::State,
    http::{HeaderMap, StatusCode},
    response::IntoResponse,
    routing::get,
};

use crate::channels::web::handlers::feature_registry::{
    DEFAULT_DEPLOYMENT_ID, deployment_id_from_headers,
};
use crate::channels::web::server::GatewayState;

/// Compiled defaults for the browser flags, mirroring
/// `web-src/axinite/src/lib/feature-flags/registry.ts`. Keep the two lists in
/// step when adding a flag.
const FLAG_DEFAULTS: &[(&str, bool)] = &[
    ("route_chat", true),
    ("route_memory", true),
    ("route_jobs", true),
    ("route_routines", true),
    ("route_extensions", true),
    ("route_skills", true),
    ("route_logs", true),
    ("panel_logs", true),
    ("action_memory_edit", false),
    ("action_job_restart", false),
    ("action_routine_trigger", false),
    ("action_extension_install", false),
    ("action_skill_install", false),
    ("surface_tee_attestation", false),
];

pub fn routes() -> Router<Arc<GatewayState>> {
    Router::new().route("/api/features", get(features_handler))
}

/// Response header carrying the gateway build version so browsers can
/// correlate flag availability with the host build without polluting the flat
/// RFC 0009 body shape.
pub const VERSION_HEADER: &str = "x-axinite-version";

/// Serve the resolved flag map for the requesting deployment.
///
/// An absent `X-Deployment-Id` header selects [`DEFAULT_DEPLOYMENT_ID`]; a
/// present but malformed one is rejected with `400 Bad Request` rather than
/// silently resolving another deployment's flags.
pub async fn features_handler(
    State(state): State<Arc<GatewayState>>,
    headers: HeaderMap,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    let deployment_id = deployment_id_from_headers(&headers)
        .map_err(|error| (StatusCode::BAD_REQUEST, error.to_string()))?
        .unwrap_or_else(|| DEFAULT_DEPLOYMENT_ID.to_string());

    // A read still answers when the store is unavailable: the unhydrated
    // deployment resolves from the environment and compiled defaults.
    if let Err(error) = ensure_deployment_hydrated(&state, &deployment_id).await {
        tracing::error!(
            deployment_id,
            %error,
            "Failed to load deployment feature-flag overrides"
        );
    }

    let overrides = state
        .feature_flags
        .read()
        .await
        .overrides_for(&deployment_id);
    let unavailable = unavailable_subsystem_flags(&state).await;

    Ok((
        [(VERSION_HEADER, env!("CARGO_PKG_VERSION"))],
        Json(resolve_flags(
            |variable| env_flag_overlay().get(variable).cloned(),
            &overrides,
            &unavailable,
        )),
    ))
}

/// Environment variable consulted for one flag: `FEATURE_FLAG_<UPPER_NAME>`.
fn flag_env_var(name: &str) -> String {
    format!("FEATURE_FLAG_{}", name.to_ascii_uppercase())
}

/// The `FEATURE_FLAG_*` environment layer, read once per process.
///
/// A running process's environment cannot be changed from outside, so
/// re-reading it on every `GET /api/features` only repeats work; the overlay
/// is captured on first use and reused for the process lifetime. Only the
/// variables for known flags are captured.
pub(super) fn env_flag_overlay() -> &'static HashMap<String, String> {
    static OVERLAY: OnceLock<HashMap<String, String>> = OnceLock::new();
    OVERLAY.get_or_init(|| {
        FLAG_DEFAULTS
            .iter()
            .filter_map(|(name, _)| {
                let variable = flag_env_var(name);
                std::env::var(&variable).ok().map(|value| (variable, value))
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
    let mut unavailable = Vec::new();

    let scheduler_present = match state.scheduler.as_ref() {
        Some(slot) => slot.read().await.is_some(),
        None => false,
    };
    if state.job_manager.is_none() && !scheduler_present {
        unavailable.extend(["route_jobs", "action_job_restart"]);
    }
    if !state.routine_engine.read().await.is_some() {
        unavailable.extend(["route_routines", "action_routine_trigger"]);
    }
    if state.extension_manager.is_none() {
        unavailable.extend(["route_extensions", "action_extension_install"]);
    }
    if state.skill_registry.is_none() {
        unavailable.extend(["route_skills", "action_skill_install"]);
    }
    if state.log_broadcaster.is_none() {
        unavailable.extend(["route_logs", "panel_logs"]);
    }

    unavailable
}

/// Ensure the registry has loaded the given deployment's overrides from the
/// store exactly once.
///
/// Lazy hydration keeps `GatewayChannel::new()` synchronous (it has no store
/// yet) while still reflecting persisted overrides after a restart. When no
/// store is wired, resolution falls back to environment variables and compiled
/// defaults, so the deployment is left un-hydrated and simply resolves from
/// defaults.
///
/// A store failure is returned and leaves the deployment un-hydrated, so the
/// next call retries; each caller decides whether it can proceed without the
/// persisted overrides.
async fn ensure_deployment_hydrated(
    state: &GatewayState,
    deployment_id: &str,
) -> Result<(), crate::error::DatabaseError> {
    if state.feature_flags.read().await.is_hydrated(deployment_id) {
        return Ok(());
    }

    let Some(store) = state.store.as_ref() else {
        return Ok(());
    };

    let overrides = store.list_deployment_flags(deployment_id).await?;
    state
        .feature_flags
        .write()
        .await
        .hydrate(deployment_id.to_string(), overrides);
    Ok(())
}

/// Resolve every known flag through the precedence chain: environment variable
/// > deployment override > subsystem-availability default > compiled default.
///
/// `unavailable` lists flags whose backing subsystem is absent; they resolve
/// to `false` unless an environment variable or operator override says
/// otherwise. Only names in [`FLAG_DEFAULTS`] are emitted; unknown override
/// names are ignored, matching RFC 0009's flag-name validation posture.
fn resolve_flags(
    env: impl Fn(&str) -> Option<String>,
    overrides: &HashMap<String, bool>,
    unavailable: &[&str],
) -> BTreeMap<String, bool> {
    FLAG_DEFAULTS
        .iter()
        .map(|(name, default)| {
            let variable = flag_env_var(name);
            let value = match env(&variable) {
                Some(raw) => raw.eq_ignore_ascii_case("true"),
                None => overrides.get(*name).copied().unwrap_or_else(|| {
                    if unavailable.contains(name) {
                        false
                    } else {
                        *default
                    }
                }),
            };
            ((*name).to_string(), value)
        })
        .collect()
}

/// Persist and cache a deployment-scoped override, then return the resolved
/// value (which may still be overridden by an environment variable).
///
/// Used by the settings handler when intercepting `feature_flag:` writes so the
/// database and the in-memory registry stay in step without a restart.
pub(crate) async fn apply_flag_override(
    state: &GatewayState,
    deployment_id: &str,
    flag_name: &str,
    enabled: bool,
) -> Result<(), crate::error::DatabaseError> {
    let store = state
        .store
        .as_ref()
        .ok_or_else(|| crate::error::DatabaseError::Query("no store configured".to_string()))?;

    // Ensure the deployment is hydrated first so the write does not create an
    // isolated, partially populated cache entry that hides other overrides.
    // If the persisted overrides cannot be loaded, neither persist nor cache
    // this one: caching it would mark the deployment hydrated without them.
    ensure_deployment_hydrated(state, deployment_id).await?;

    store
        .set_deployment_flag(deployment_id, flag_name, enabled)
        .await?;

    state.feature_flags.write().await.set(
        deployment_id.to_string(),
        flag_name.to_string(),
        enabled,
    );

    Ok(())
}

#[cfg(test)]
mod tests;
