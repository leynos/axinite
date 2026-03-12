//! Shared test-only utilities for env-mutation guards and similar fixtures.

use std::sync::MutexGuard;

use crate::config::helpers::ENV_MUTEX;

/// Guard that snapshots a set of env vars, serializes mutations under
/// [`ENV_MUTEX`], and restores the original values on drop.
pub struct EnvVarsGuard {
    _lock: MutexGuard<'static, ()>,
    originals: Vec<(&'static str, Option<String>)>,
}

impl EnvVarsGuard {
    /// Lock env access for the duration of the guard and snapshot the keys.
    pub fn new(keys: &[&'static str]) -> Self {
        let lock = ENV_MUTEX.lock().expect("env mutex poisoned");
        let originals = keys
            .iter()
            .map(|key| (*key, std::env::var(key).ok()))
            .collect();
        Self {
            _lock: lock,
            originals,
        }
    }

    /// Set an env var while the shared env lock is held.
    pub fn set(&self, key: &str, value: &str) {
        // SAFETY: EnvVarsGuard holds ENV_MUTEX for its entire lifetime.
        unsafe {
            std::env::set_var(key, value);
        }
    }

    /// Remove an env var while the shared env lock is held.
    pub fn remove(&self, key: &str) {
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
