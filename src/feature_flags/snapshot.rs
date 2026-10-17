//! Typed operator values and pure override transitions, without cache or I/O state.

use super::{DeploymentId, FlagName};
use std::collections::BTreeMap;

/// A detached snapshot of validated operator flag values.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct FlagSnapshot {
    values: BTreeMap<FlagName, bool>,
}

impl FlagSnapshot {
    /// Construct a snapshot from validated names; later duplicate names win.
    pub fn from_values(values: impl IntoIterator<Item = (FlagName, bool)>) -> Self {
        Self {
            values: values.into_iter().collect(),
        }
    }

    /// Read an override without changing the snapshot.
    pub fn get(&self, name: &str) -> Option<bool> {
        self.values.get(name).copied()
    }

    /// Replace a single validated value in this local snapshot.
    pub fn set(&mut self, name: FlagName, enabled: bool) {
        self.values.insert(name, enabled);
    }

    /// Iterate over detached operator values in deterministic name order.
    pub fn values(&self) -> impl Iterator<Item = (&FlagName, bool)> {
        self.values.iter().map(|(name, enabled)| (name, *enabled))
    }

    /// Number of operator overrides in this snapshot.
    pub fn len(&self) -> usize {
        self.values.len()
    }

    /// Whether this snapshot contains no operator overrides.
    pub fn is_empty(&self) -> bool {
        self.values.is_empty()
    }
}

/// A validated operator command; it does not perform persistence itself.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FlagOverrideCommand {
    /// The deployment whose operator override changes.
    pub deployment: DeploymentId,
    /// Syntactically validated flag name.
    pub flag: FlagName,
    /// Desired operator value; environment precedence still applies.
    pub enabled: bool,
}
