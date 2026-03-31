//! Live WASM channel and channel-relay activation adapter.
//!
//! Currently delegates to the [`ExtensionManager`]'s internal methods via
//! a `Weak` reference to avoid a retain cycle. The channel activation
//! logic is deeply coupled to manager state (active-channel tracking,
//! credential refresh, webhook router registration, auth-status checks) and
//! will be extracted incrementally in a follow-up.
//!
//! The port seam is in place so that tests can inject
//! [`NoOpWasmChannelActivation`](super::NoOpWasmChannelActivation) without
//! triggering real channel infrastructure.

use std::sync::{Arc, OnceLock, Weak};

use crate::extensions::activation::{ActivationFuture, WasmChannelActivationPort};
use crate::extensions::{ExtensionError, ExtensionManager};

/// Live adapter that delegates channel activation to the
/// [`ExtensionManager`]'s existing methods.
///
/// Set post-construction via [`Self::set_manager`] once the manager is
/// wrapped in `Arc`.
pub struct LiveWasmChannelActivation {
    /// Populated after `ExtensionManager` is wrapped in `Arc`.
    /// Stored as `Weak` to avoid a retain cycle with the manager.
    manager: OnceLock<Weak<ExtensionManager>>,
}

impl LiveWasmChannelActivation {
    /// Create an uninitialized adapter. Call [`Self::set_manager`] before use.
    pub fn new() -> Self {
        Self {
            manager: OnceLock::new(),
        }
    }

    /// Inject the manager reference once it is available.
    pub fn set_manager(&self, manager: Arc<ExtensionManager>) {
        match self.manager.set(Arc::downgrade(&manager)) {
            Ok(_) => {}
            Err(_) => {
                tracing::debug!(
                    "LiveWasmChannelActivation::set_manager called twice; ignoring second call"
                );
            }
        }
    }

    fn require_manager(&self) -> Result<Arc<ExtensionManager>, ExtensionError> {
        self.manager.get().and_then(|w| w.upgrade()).ok_or_else(|| {
            ExtensionError::ActivationFailed(
                "Channel activation adapter not initialised".to_string(),
            )
        })
    }
}

impl Default for LiveWasmChannelActivation {
    fn default() -> Self {
        Self::new()
    }
}

impl WasmChannelActivationPort for LiveWasmChannelActivation {
    fn activate_wasm_channel<'a>(&'a self, name: &'a str) -> ActivationFuture<'a> {
        let mgr = self.require_manager();
        let name = name.to_owned();
        Box::pin(async move { mgr?.activate_wasm_channel_inner(&name).await })
    }

    fn activate_channel_relay<'a>(&'a self, name: &'a str) -> ActivationFuture<'a> {
        let mgr = self.require_manager();
        let name = name.to_owned();
        Box::pin(async move { mgr?.activate_channel_relay_inner(&name).await })
    }
}
