//! Live WASM channel and channel-relay activation adapter.
//!
//! Currently delegates to the [`ExtensionManager`]'s internal methods via
//! an `Arc` reference. This is an intermediate step — the channel activation
//! logic is deeply coupled to manager state (active-channel tracking,
//! credential refresh, webhook router registration, auth-status checks) and
//! will be extracted incrementally in a follow-up.
//!
//! The port seam is in place so that tests can inject
//! [`NoOpWasmChannelActivation`](super::NoOpWasmChannelActivation) without
//! triggering real channel infrastructure.

use std::sync::Arc;

use tokio::sync::RwLock;

use crate::extensions::activation::NativeWasmChannelActivationPort;
use crate::extensions::{ActivateResult, ExtensionError, ExtensionManager};

/// Live adapter that delegates channel activation to the
/// [`ExtensionManager`]'s existing methods.
///
/// Set post-construction via
/// [`ExtensionManager::set_wasm_channel_activation`] once the manager is
/// wrapped in `Arc`.
pub struct LiveWasmChannelActivation {
    /// Populated after `ExtensionManager` is wrapped in `Arc`.
    manager: RwLock<Option<Arc<ExtensionManager>>>,
}

impl LiveWasmChannelActivation {
    /// Create an uninitialised adapter. Call [`Self::set_manager`] before use.
    pub fn new() -> Self {
        Self {
            manager: RwLock::new(None),
        }
    }

    /// Inject the manager reference once it is available.
    pub async fn set_manager(&self, manager: Arc<ExtensionManager>) {
        *self.manager.write().await = Some(manager);
    }

    async fn require_manager(&self) -> Result<Arc<ExtensionManager>, ExtensionError> {
        self.manager.read().await.clone().ok_or_else(|| {
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

impl NativeWasmChannelActivationPort for LiveWasmChannelActivation {
    async fn activate_wasm_channel<'a>(
        &'a self,
        name: &'a str,
    ) -> Result<ActivateResult, ExtensionError> {
        let mgr = self.require_manager().await?;
        mgr.activate_wasm_channel_inner(name).await
    }

    async fn activate_channel_relay<'a>(
        &'a self,
        name: &'a str,
    ) -> Result<ActivateResult, ExtensionError> {
        let mgr = self.require_manager().await?;
        mgr.activate_channel_relay_inner(name).await
    }
}
