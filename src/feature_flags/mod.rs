//! Pure feature-flag identities, snapshots, and resolution policy; all I/O belongs to adapters.

use std::collections::BTreeMap;

pub mod availability;
mod snapshot;
pub use snapshot::{FlagOverrideCommand, FlagSnapshot};
mod types;
pub use types::{
    DeploymentId, FlagName, InvalidDeploymentId, InvalidFlagName, MAX_DEPLOYMENT_ID_LEN,
};

/// Compiled defaults for the browser flags, mirroring
/// `web-src/axinite/src/lib/feature-flags/registry.ts`. Keep the two lists in
/// step when adding a flag.
pub const FLAG_DEFAULTS: &[(&str, bool)] = &[
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

/// Resolve every known flag through the precedence chain: environment variable
/// > deployment override > subsystem-availability default > compiled default.
///
/// `env` supplies already-parsed environment values from the driving adapter.
/// `unavailable` lists flags whose backing subsystem is
/// absent; they resolve to `false` unless an environment variable or operator
/// override says otherwise. Only names in [`FLAG_DEFAULTS`] are emitted;
/// unknown override names are ignored, matching RFC 0009's flag-name
/// validation posture.
pub fn resolve_flags(
    env: impl Fn(&str) -> Option<bool>,
    overrides: &FlagSnapshot,
    unavailable: &[&str],
) -> BTreeMap<String, bool> {
    FLAG_DEFAULTS
        .iter()
        .map(|(name, default)| {
            let value = env(name).unwrap_or_else(|| {
                overrides.get(name).unwrap_or_else(|| {
                    if unavailable.contains(name) {
                        false
                    } else {
                        *default
                    }
                })
            });
            ((*name).to_string(), value)
        })
        .collect()
}

#[cfg(test)]
mod tests;
