//! Config-overlay isolation for wizard tests.

/// RAII guard for injected config overlay entries used by auth-resolution tests.
pub(super) struct OverlayGuard {
    key: &'static str,
}

impl OverlayGuard {
    pub(super) fn set(key: &'static str, value: &str) -> Self {
        crate::config::remove_single_var(key);
        crate::config::inject_single_var(key, value);
        Self { key }
    }
}

impl Drop for OverlayGuard {
    fn drop(&mut self) {
        crate::config::remove_single_var(self.key);
    }
}
