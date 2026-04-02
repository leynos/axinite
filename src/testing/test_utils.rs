//! Shared test utilities for env-mutation guards and config snapshots.

use std::collections::HashMap;

use crate::config::helpers::{ENV_MUTEX, EnvMutexGuard};
use crate::config::{Config, EnvContext};
use crate::settings::Settings;

/// Guard that snapshots a set of env vars, serializes mutations under
/// [`ENV_MUTEX`], and restores the original values on drop.
pub struct EnvVarsGuard {
    _lock: EnvMutexGuard<'static>,
    originals: HashMap<String, Option<String>>,
}

impl EnvVarsGuard {
    /// Lock env access for the duration of the guard and snapshot the keys.
    pub fn new(keys: &[&'static str]) -> Self {
        let lock = ENV_MUTEX.lock().expect("env mutex poisoned");
        let originals = keys
            .iter()
            .map(|key| ((*key).to_string(), std::env::var(key).ok()))
            .collect();
        Self {
            _lock: lock,
            originals,
        }
    }

    fn snapshot_key_if_needed(&mut self, key: &str) {
        self.originals
            .entry(key.to_string())
            .or_insert_with(|| std::env::var(key).ok());
    }

    /// Set an env var while the shared env lock is held.
    pub fn set(&mut self, key: &str, value: &str) {
        self.snapshot_key_if_needed(key);
        // SAFETY: EnvVarsGuard holds ENV_MUTEX for its entire lifetime.
        unsafe {
            std::env::set_var(key, value);
        }
    }

    /// Remove an env var while the shared env lock is held.
    pub fn remove(&mut self, key: &str) {
        self.snapshot_key_if_needed(key);
        // SAFETY: EnvVarsGuard holds ENV_MUTEX for its entire lifetime.
        unsafe {
            std::env::remove_var(key);
        }
    }
}

impl Drop for EnvVarsGuard {
    fn drop(&mut self) {
        for (key, value) in &self.originals {
            // SAFETY: EnvVarsGuard holds ENV_MUTEX for its entire lifetime.
            unsafe {
                if let Some(value) = value {
                    std::env::set_var(key, value);
                } else {
                    std::env::remove_var(key);
                }
            }
        }
    }
}

/// Create an empty explicit config context for tests.
///
/// Use this when a test wants to build configuration without mutating the
/// host process environment or depending on machine-local credentials.
pub fn test_env_context() -> EnvContext {
    EnvContext::default()
}

/// Create a test context populated with explicit env values.
///
/// This is the preferred helper for tests that need to express precedence
/// through concrete env-var inputs while keeping resolution deterministic.
pub fn test_env_context_with(vars: &[(&str, &str)]) -> EnvContext {
    vars.iter()
        .fold(EnvContext::default(), |ctx, (key, value)| {
            ctx.with_env(*key, *value)
        })
}

/// Build a config from an explicit test context.
///
/// The config is resolved through [`Config::from_context`] using
/// [`Settings::default`], so tests can exercise the explicit snapshot path
/// without touching ambient state.
pub async fn test_config_from_context(
    ctx: &EnvContext,
) -> Result<Config, crate::error::ConfigError> {
    Config::from_context(ctx, &Settings::default()).await
}
