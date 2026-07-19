//! Deployment feature flags for the browser UI.
//!
//! Implements the RFC 0009 delivery mechanism: the resolved flag map is exposed
//! at `GET /api/features`. Each flag resolves through the precedence chain
//!
//! 1. `FEATURE_FLAG_<UPPER_SNAKE_NAME>` environment variable (highest),
//! 2. deployment-scoped operator override (from the registry / store),
//! 3. compiled default (lowest).
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
use std::sync::Arc;

use axum::{Json, Router, extract::State, http::HeaderMap, routing::get};

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

pub async fn features_handler(
    State(state): State<Arc<GatewayState>>,
    headers: HeaderMap,
) -> Json<BTreeMap<String, bool>> {
    let deployment_id =
        deployment_id_from_headers(&headers).unwrap_or_else(|| DEFAULT_DEPLOYMENT_ID.to_string());

    ensure_deployment_hydrated(&state, &deployment_id).await;

    let overrides = state
        .feature_flags
        .read()
        .await
        .overrides_for(&deployment_id);

    Json(resolve_flags(|name| std::env::var(name).ok(), &overrides))
}

/// Ensure the registry has loaded the given deployment's overrides from the
/// store exactly once.
///
/// Lazy hydration keeps `GatewayChannel::new()` synchronous (it has no store
/// yet) while still reflecting persisted overrides after a restart. When no
/// store is wired, resolution falls back to environment variables and compiled
/// defaults, so the deployment is left un-hydrated and simply resolves from
/// defaults.
async fn ensure_deployment_hydrated(state: &GatewayState, deployment_id: &str) {
    if state.feature_flags.read().await.is_hydrated(deployment_id) {
        return;
    }

    let Some(store) = state.store.as_ref() else {
        return;
    };

    match store.list_deployment_flags(deployment_id).await {
        Ok(overrides) => {
            state
                .feature_flags
                .write()
                .await
                .hydrate(deployment_id.to_string(), overrides);
        }
        Err(error) => {
            tracing::error!(
                deployment_id,
                %error,
                "Failed to load deployment feature-flag overrides"
            );
        }
    }
}

/// Resolve every known flag through the precedence chain: environment variable
/// > deployment override > compiled default.
///
/// Only names in [`FLAG_DEFAULTS`] are emitted; unknown override names are
/// ignored, matching RFC 0009's flag-name validation posture.
fn resolve_flags(
    env: impl Fn(&str) -> Option<String>,
    overrides: &HashMap<String, bool>,
) -> BTreeMap<String, bool> {
    FLAG_DEFAULTS
        .iter()
        .map(|(name, default)| {
            let variable = format!("FEATURE_FLAG_{}", name.to_ascii_uppercase());
            let value = match env(&variable) {
                Some(raw) => raw.eq_ignore_ascii_case("true"),
                None => overrides.get(*name).copied().unwrap_or(*default),
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
    ensure_deployment_hydrated(state, deployment_id).await;

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
mod tests {
    //! Unit tests for feature-flag resolution.

    use super::*;

    fn no_overrides() -> HashMap<String, bool> {
        HashMap::new()
    }

    #[test]
    fn defaults_apply_when_no_environment_or_override_exists() {
        let flags = resolve_flags(|_| None, &no_overrides());
        assert_eq!(flags.get("route_chat"), Some(&true));
        assert_eq!(flags.get("panel_logs"), Some(&true));
        assert_eq!(flags.get("action_memory_edit"), Some(&false));
        assert_eq!(flags.len(), FLAG_DEFAULTS.len());
    }

    #[test]
    fn environment_variables_override_defaults() {
        let flags = resolve_flags(
            |name| match name {
                "FEATURE_FLAG_ACTION_MEMORY_EDIT" => Some("TRUE".to_string()),
                "FEATURE_FLAG_ROUTE_SKILLS" => Some("false".to_string()),
                _ => None,
            },
            &no_overrides(),
        );
        assert_eq!(flags.get("action_memory_edit"), Some(&true));
        assert_eq!(flags.get("route_skills"), Some(&false));
        // Untouched flags keep their compiled defaults.
        assert_eq!(flags.get("route_chat"), Some(&true));
    }

    #[test]
    fn non_true_values_disable_the_flag() {
        let flags = resolve_flags(
            |name| (name == "FEATURE_FLAG_ROUTE_CHAT").then(|| "1".to_string()),
            &no_overrides(),
        );
        assert_eq!(flags.get("route_chat"), Some(&false));
    }

    #[test]
    fn deployment_override_beats_compiled_default() {
        let mut overrides = HashMap::new();
        overrides.insert("panel_logs".to_string(), false);
        overrides.insert("action_job_restart".to_string(), true);
        let flags = resolve_flags(|_| None, &overrides);
        assert_eq!(flags.get("panel_logs"), Some(&false));
        assert_eq!(flags.get("action_job_restart"), Some(&true));
        // A flag with no override keeps its default.
        assert_eq!(flags.get("route_chat"), Some(&true));
    }

    #[test]
    fn environment_variable_beats_deployment_override() {
        let mut overrides = HashMap::new();
        overrides.insert("route_chat".to_string(), false);
        let flags = resolve_flags(
            |name| (name == "FEATURE_FLAG_ROUTE_CHAT").then(|| "true".to_string()),
            &overrides,
        );
        // Env var wins over the override.
        assert_eq!(flags.get("route_chat"), Some(&true));
    }

    #[test]
    fn unknown_override_names_are_ignored() {
        let mut overrides = HashMap::new();
        overrides.insert("not_a_real_flag".to_string(), true);
        let flags = resolve_flags(|_| None, &overrides);
        assert!(!flags.contains_key("not_a_real_flag"));
        assert_eq!(flags.len(), FLAG_DEFAULTS.len());
    }
}
