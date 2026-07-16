//! RAII guards for environment-variable and config-overlay isolation in
//! wizard tests.

use crate::config::helpers::ENV_MUTEX;

/// RAII guard that sets/clears an env var for the duration of a test.
pub(super) struct EnvGuard {
    _lock: crate::config::helpers::EnvMutexGuard<'static>,
    key: &'static str,
    original: Option<String>,
}

impl EnvGuard {
    pub(super) fn new_with_action(key: &'static str, f: impl FnOnce()) -> Self {
        // The env mutex lock is infallible (its error type is `Infallible`).
        let Ok(lock) = ENV_MUTEX.lock();
        let original = std::env::var(key).ok();
        // SAFETY: Tests hold ENV_MUTEX for the full guard lifetime, so no
        // concurrent env mutation can occur while this override is active.
        f();
        Self {
            _lock: lock,
            key,
            original,
        }
    }

    pub(super) fn clear(key: &'static str) -> Self {
        Self::new_with_action(key, || unsafe { std::env::remove_var(key) })
    }
}

impl Drop for EnvGuard {
    fn drop(&mut self) {
        unsafe {
            if let Some(ref val) = self.original {
                std::env::set_var(self.key, val);
            } else {
                std::env::remove_var(self.key);
            }
        }
    }
}

/// RAII guard that updates multiple env vars under a single test mutex.
pub(super) struct EnvBatchGuard {
    _lock: crate::config::helpers::EnvMutexGuard<'static>,
    originals: Vec<(&'static str, Option<String>)>,
}

impl EnvBatchGuard {
    pub(super) fn new(updates: &[(&'static str, Option<&str>)]) -> Self {
        // The env mutex lock is infallible (its error type is `Infallible`).
        let Ok(lock) = ENV_MUTEX.lock();
        let originals = updates
            .iter()
            .map(|(key, _)| (*key, std::env::var(key).ok()))
            .collect::<Vec<_>>();

        for (key, value) in updates {
            // SAFETY: Tests hold ENV_MUTEX for the full batch-guard
            // lifetime, so these mutations remain serialized.
            unsafe {
                if let Some(value) = value {
                    std::env::set_var(key, value);
                } else {
                    std::env::remove_var(key);
                }
            }
        }

        Self {
            _lock: lock,
            originals,
        }
    }
}

impl Drop for EnvBatchGuard {
    fn drop(&mut self) {
        for (key, original) in &self.originals {
            // SAFETY: Tests still hold ENV_MUTEX during restoration, so
            // the env is restored without concurrent mutation.
            unsafe {
                if let Some(value) = original {
                    std::env::set_var(key, value);
                } else {
                    std::env::remove_var(key);
                }
            }
        }
    }
}

/// RAII guard for injected config overlay entries used by auth-resolution tests.
pub(super) struct OverlayGuard {
    _lock: crate::config::helpers::EnvMutexGuard<'static>,
    key: &'static str,
}

impl OverlayGuard {
    pub(super) fn set(key: &'static str, value: &str) -> Self {
        // The env mutex lock is infallible (its error type is `Infallible`).
        let Ok(lock) = ENV_MUTEX.lock();
        crate::config::remove_single_var(key);
        crate::config::inject_single_var(key, value);
        Self { _lock: lock, key }
    }
}

impl Drop for OverlayGuard {
    fn drop(&mut self) {
        crate::config::remove_single_var(self.key);
    }
}
