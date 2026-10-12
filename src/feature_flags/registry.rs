//! Deployment-scoped override cache; it never stores resolved policy values.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use tokio::sync::Mutex;

use super::DeploymentId;

/// A mutable registry of deployment-scoped feature-flag overrides.
///
/// Partial writes are separate from hydration state, so they cannot masquerade
/// as a complete store snapshot.
#[derive(Debug, Default)]
pub struct FeatureFlagRegistry {
    /// Cached override states: deployment -> (name -> enabled).
    flags: HashMap<String, HashMap<String, bool>>,
    hydrated: HashSet<String>,
    failed: HashSet<String>,
    /// Admission gates share the deployment hydration unit, not individual flags.
    operation_gates: HashMap<DeploymentId, Arc<Mutex<()>>>,
}

impl FeatureFlagRegistry {
    /// Share one operation gate for a deployment without retaining a cache lock.
    ///
    /// The service owns admission across hydration, persistence, and publication;
    /// another deployment receives a separate gate and can progress independently.
    pub(super) fn operation_gate(&mut self, deployment: &DeploymentId) -> Arc<Mutex<()>> {
        Arc::clone(
            self.operation_gates
                .entry(deployment.clone())
                .or_insert_with(|| Arc::new(Mutex::new(()))),
        )
    }

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
    pub fn set(&mut self, deployment_id: String, name: String, enabled: bool) {
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
        self.hydrated.contains(deployment_id)
    }

    /// Cache a deployment's overrides loaded from the store.
    ///
    /// Marks the deployment as hydrated even when `overrides` is empty.
    /// Hydration is additive: a flag already cached (for example by a
    /// [`set`](Self::set) that landed while the store query was in flight) is
    /// newer than the snapshot and is kept.
    pub fn hydrate(&mut self, deployment_id: String, overrides: Vec<(String, bool)>) {
        self.hydrated.insert(deployment_id.clone());
        self.failed.remove(&deployment_id);
        let entry = self.flags.entry(deployment_id).or_default();
        for (name, enabled) in overrides {
            entry.entry(name).or_insert(enabled);
        }
    }

    /// Return a copy of a deployment's cached overrides, if any.
    pub fn overrides_for(&self, deployment_id: &str) -> HashMap<String, bool> {
        self.flags.get(deployment_id).cloned().unwrap_or_default()
    }
}

impl FeatureFlagRegistry {
    /// Record a failed hydration without treating its partial cache as loaded.
    pub fn hydration_failed(&mut self, deployment_id: &str) {
        self.failed.insert(deployment_id.to_owned());
    }

    /// Whether the next hydration is a retry after a store failure.
    pub fn is_retry(&self, deployment_id: &str) -> bool {
        self.failed.contains(deployment_id)
    }
}

#[cfg(test)]
mod tests {
    //! Cache invariants independent of the HTTP adapter.
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

    #[test]
    fn hydrate_keeps_overrides_set_while_the_store_query_was_in_flight() {
        let mut registry = FeatureFlagRegistry::new();
        registry.set("production".to_string(), "panel_logs".to_string(), true);
        registry.hydrate(
            "production".to_string(),
            vec![
                ("panel_logs".to_string(), false),
                ("route_chat".to_string(), false),
            ],
        );
        assert_eq!(registry.get("production", "panel_logs"), Some(true));
        assert_eq!(registry.get("production", "route_chat"), Some(false));
    }
}
