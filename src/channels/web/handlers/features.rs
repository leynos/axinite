//! Deployment feature flags for the browser UI.
//!
//! Minimal implementation of the RFC 0009 delivery mechanism: the resolved
//! flag map is exposed at `GET /api/features`, and each flag can be
//! overridden with a `FEATURE_FLAG_<UPPER_SNAKE_NAME>` environment variable
//! (the value `true`, case-insensitively, enables the flag; any other set
//! value disables it; unset falls through to the compiled default). The
//! settings-table override layer and deployment scoping proposed by RFC 0009
//! are not implemented yet.

use std::collections::BTreeMap;
use std::sync::Arc;

use axum::{Json, Router, routing::get};

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

pub async fn features_handler() -> Json<BTreeMap<String, bool>> {
    Json(resolve_flags(|name| std::env::var(name).ok()))
}

/// Resolve every known flag through the environment lookup, falling back to
/// the compiled default.
fn resolve_flags(env: impl Fn(&str) -> Option<String>) -> BTreeMap<String, bool> {
    FLAG_DEFAULTS
        .iter()
        .map(|(name, default)| {
            let variable = format!("FEATURE_FLAG_{}", name.to_ascii_uppercase());
            let value = match env(&variable) {
                Some(raw) => raw.eq_ignore_ascii_case("true"),
                None => *default,
            };
            ((*name).to_string(), value)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    //! Unit tests for feature-flag resolution.

    use super::*;

    #[test]
    fn defaults_apply_when_no_environment_override_exists() {
        let flags = resolve_flags(|_| None);
        assert_eq!(flags.get("route_chat"), Some(&true));
        assert_eq!(flags.get("panel_logs"), Some(&true));
        assert_eq!(flags.get("action_memory_edit"), Some(&false));
        assert_eq!(flags.len(), FLAG_DEFAULTS.len());
    }

    #[test]
    fn environment_variables_override_defaults() {
        let flags = resolve_flags(|name| match name {
            "FEATURE_FLAG_ACTION_MEMORY_EDIT" => Some("TRUE".to_string()),
            "FEATURE_FLAG_ROUTE_SKILLS" => Some("false".to_string()),
            _ => None,
        });
        assert_eq!(flags.get("action_memory_edit"), Some(&true));
        assert_eq!(flags.get("route_skills"), Some(&false));
        // Untouched flags keep their compiled defaults.
        assert_eq!(flags.get("route_chat"), Some(&true));
    }

    #[test]
    fn non_true_values_disable_the_flag() {
        let flags =
            resolve_flags(|name| (name == "FEATURE_FLAG_ROUTE_CHAT").then(|| "1".to_string()));
        assert_eq!(flags.get("route_chat"), Some(&false));
    }
}
