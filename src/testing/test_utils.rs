//! Shared test-only utilities for env-mutation guards and similar fixtures.

use std::collections::HashMap;
use std::sync::MutexGuard;

use crate::config::helpers::ENV_MUTEX;

/// Guard that snapshots a set of env vars, serializes mutations under
/// [`ENV_MUTEX`], and restores the original values on drop.
pub struct EnvVarsGuard {
    _lock: MutexGuard<'static, ()>,
    originals: HashMap<String, Option<String>>,
}

impl EnvVarsGuard {
    /// Lock env access for the duration of the guard and snapshot the keys.
    pub fn new(keys: &[&'static str]) -> Self {
        let lock = ENV_MUTEX
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
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
