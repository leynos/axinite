//! Gateway-owned cache, retries, and admission; pure flag values live in the domain.

use crate::feature_flags::{DeploymentId, FlagOverrideCommand, FlagSnapshot};
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use tokio::sync::Mutex;

/// In-memory gateway cache populated only by explicit override commands.
#[derive(Debug, Default)]
pub struct FeatureFlagRegistry {
    flags: HashMap<DeploymentId, FlagSnapshot>,
    failed: HashSet<DeploymentId>,
    operation_gates: HashMap<DeploymentId, Arc<Mutex<()>>>,
}

/// Resource counts suitable for fixed-name gauges, without identifier labels.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub(crate) struct CacheResources {
    /// Deployments with a complete command-published snapshot.
    pub deployments: usize,
    /// Operator values retained across all published snapshots.
    pub overrides: usize,
    /// Explicit commands awaiting a healthy hydration retry.
    pub failed_hydrations: usize,
    /// Deployment admission gates retained for command ordering.
    pub admission_gates: usize,
}

impl FeatureFlagRegistry {
    /// Construct an empty gateway cache and command admission registry.
    pub fn new() -> Self {
        Self::default()
    }

    /// Read a cached value without changing command or hydration state.
    pub fn get(&self, deployment: &str, flag: &str) -> Option<bool> {
        self.flags
            .get(deployment)
            .and_then(|snapshot| snapshot.get(flag))
    }

    /// Whether a command has published a complete persisted snapshot.
    pub fn is_hydrated(&self, deployment: &str) -> bool {
        self.flags.contains_key(deployment)
    }

    /// Detached cached command snapshot; query callers cannot mutate shared values.
    pub(crate) fn snapshot(&self, deployment: &DeploymentId) -> Option<FlagSnapshot> {
        self.flags.get(deployment).cloned()
    }

    /// Publish a complete snapshot under command admission after a healthy load.
    pub(super) fn hydrate(&mut self, deployment: DeploymentId, snapshot: FlagSnapshot) {
        self.failed.remove(&deployment);
        self.flags.insert(deployment, snapshot);
    }

    /// Publish an operator value only after its persistence succeeds.
    pub(super) fn set(&mut self, command: &FlagOverrideCommand) {
        self.flags
            .entry(command.deployment.clone())
            .or_default()
            .set(command.flag.clone(), command.enabled);
    }

    /// Preserve retry eligibility after an explicit command's failed load.
    pub(super) fn hydration_failed(&mut self, deployment: &DeploymentId) {
        self.failed.insert(deployment.clone());
    }

    /// Whether an explicit command is retrying an earlier hydration failure.
    pub(crate) fn is_retry(&self, deployment: &str) -> bool {
        self.failed.contains(deployment)
    }

    /// Clone the deployment admission gate under a brief registry lock.
    pub(super) fn operation_gate(&mut self, deployment: &DeploymentId) -> Arc<Mutex<()>> {
        Arc::clone(
            self.operation_gates
                .entry(deployment.clone())
                .or_insert_with(|| Arc::new(Mutex::new(()))),
        )
    }

    /// Report cache growth without exposing deployment or flag identifiers.
    pub(crate) fn resources(&self) -> CacheResources {
        CacheResources {
            deployments: self.flags.len(),
            overrides: self.flags.values().map(FlagSnapshot::len).sum(),
            failed_hydrations: self.failed.len(),
            admission_gates: self.operation_gates.len(),
        }
    }
}

#[cfg(test)]
mod tests {
    //! Gateway cache publication and count regressions independent of storage.
    use super::*;
    use crate::feature_flags::FlagName;

    #[test]
    fn command_hydration_and_publication_keep_resource_counts_coherent() {
        let deployment = DeploymentId::parse("production").expect("valid deployment");
        let mut cache = FeatureFlagRegistry::new();
        cache.hydration_failed(&deployment);
        assert_eq!(cache.resources().failed_hydrations, 1);
        cache.hydrate(deployment.clone(), FlagSnapshot::default());
        assert_eq!(cache.resources().deployments, 1);
        assert_eq!(cache.resources().failed_hydrations, 0);
        let command = FlagOverrideCommand {
            deployment,
            flag: FlagName::parse("panel_logs").expect("valid flag"),
            enabled: false,
        };
        cache.set(&command);
        assert_eq!(cache.resources().overrides, 1);
        assert_eq!(cache.get("production", "panel_logs"), Some(false));
        assert_eq!(cache.get("staging", "panel_logs"), None);
    }
}
