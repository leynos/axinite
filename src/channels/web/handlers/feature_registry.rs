//! In-memory registry of deployment-scoped feature-flag overrides (RFC 0009).
//!
//! The registry caches the operator overrides persisted in
//! `feature_flag_overrides`, keyed by deployment. It holds only the override
//! layer, not fully resolved flag values: resolution (environment variable >
//! deployment override > compiled default) happens in
//! [`super::features`] when serving `GET /api/features`.
//!
//! The registry is held in `GatewayState` behind an `Arc<RwLock<..>>` so
//! handlers can read it on the hot path and update it synchronously when an
//! operator writes an override through the settings API. Writes update both the
//! database and this registry, so the effect is visible on the next
//! `GET /api/features` without a restart.
//!
//! Hydration is lazy: on the first read for a deployment that has not yet been
//! loaded, [`super::features`] queries the store once and caches the overrides
//! here (see `ensure_deployment_hydrated`). This avoids threading an async
//! store load through the synchronous `GatewayChannel::new()` construction path.

use std::collections::HashMap;

use axum::http::HeaderMap;

/// A deployment identifier (for example `"production"` or `"default"`).
pub type DeploymentId = String;

/// Header carrying the deployment identifier for feature-flag reads and writes.
///
/// Reads (`GET /api/features`) treat it as optional and fall back to
/// [`DEFAULT_DEPLOYMENT_ID`]; writes (`PUT /api/settings/feature_flag:<name>`)
/// require it.
pub const DEPLOYMENT_ID_HEADER: &str = "x-deployment-id";

/// Deployment used when the `X-Deployment-Id` header is absent on reads.
pub const DEFAULT_DEPLOYMENT_ID: &str = "default";

/// Extract a trimmed, non-empty deployment identifier from request headers.
///
/// Returns `None` when the header is absent, empty, whitespace-only, or not
/// valid UTF-8.
pub fn deployment_id_from_headers(headers: &HeaderMap) -> Option<String> {
    headers
        .get(DEPLOYMENT_ID_HEADER)
        .and_then(|value| value.to_str().ok())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

/// A mutable registry of deployment-scoped feature-flag overrides.
///
/// Maps deployment -> (flag name -> enabled). Presence of a deployment key
/// means it has been hydrated from the store, even if it has no overrides.
#[derive(Debug, Default)]
pub struct FeatureFlagRegistry {
    /// Cached override states: deployment -> (name -> enabled).
    flags: HashMap<DeploymentId, HashMap<String, bool>>,
}

impl FeatureFlagRegistry {
    /// Create an empty registry.
    pub fn new() -> Self {
        Self::default()
    }

    /// Return the override for a single flag, if one is cached.
    pub fn get(&self, deployment_id: &str, name: &str) -> Option<bool> {
        self.flags
            .get(deployment_id)
            .and_then(|deployment_flags| deployment_flags.get(name).copied())
    }

    /// Insert or replace one deployment-scoped override.
    pub fn set(&mut self, deployment_id: DeploymentId, name: String, enabled: bool) {
        self.flags
            .entry(deployment_id)
            .or_default()
            .insert(name, enabled);
    }

    /// Whether a deployment's overrides have been loaded from the store.
    ///
    /// Returns `true` once the deployment has been hydrated (including when it
    /// has no overrides), so callers can skip a repeat store query.
    pub fn is_hydrated(&self, deployment_id: &str) -> bool {
        self.flags.contains_key(deployment_id)
    }

    /// Cache a deployment's overrides loaded from the store.
    ///
    /// Marks the deployment as hydrated even when `overrides` is empty.
    pub fn hydrate(&mut self, deployment_id: DeploymentId, overrides: Vec<(String, bool)>) {
        let entry = self.flags.entry(deployment_id).or_default();
        for (name, enabled) in overrides {
            entry.insert(name, enabled);
        }
    }

    /// Return a copy of a deployment's cached overrides, if any.
    pub fn overrides_for(&self, deployment_id: &str) -> HashMap<String, bool> {
        self.flags.get(deployment_id).cloned().unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    //! Unit tests for the deployment-scoped feature-flag registry.

    use super::*;

    #[test]
    fn get_returns_none_for_unknown_deployment_or_flag() {
        let mut registry = FeatureFlagRegistry::new();
        assert_eq!(registry.get("production", "route_chat"), None);
        registry.set("production".to_string(), "route_chat".to_string(), true);
        assert_eq!(registry.get("production", "route_chat"), Some(true));
        assert_eq!(registry.get("production", "unknown"), None);
        assert_eq!(registry.get("staging", "route_chat"), None);
    }

    #[test]
    fn hydrate_marks_deployment_loaded_even_when_empty() {
        let mut registry = FeatureFlagRegistry::new();
        assert!(!registry.is_hydrated("default"));
        registry.hydrate("default".to_string(), vec![]);
        assert!(registry.is_hydrated("default"));
        assert!(registry.overrides_for("default").is_empty());
    }

    #[test]
    fn hydrate_populates_and_set_overwrites() {
        let mut registry = FeatureFlagRegistry::new();
        registry.hydrate(
            "production".to_string(),
            vec![("panel_logs".to_string(), false)],
        );
        assert_eq!(registry.get("production", "panel_logs"), Some(false));
        registry.set("production".to_string(), "panel_logs".to_string(), true);
        assert_eq!(registry.get("production", "panel_logs"), Some(true));

        let overrides = registry.overrides_for("production");
        assert_eq!(overrides.get("panel_logs"), Some(&true));
    }
}
