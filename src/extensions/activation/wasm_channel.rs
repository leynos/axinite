//! WASM channel activation port.
//!
//! Isolates WASM channel loading, credential injection, webhook router
//! registration, and hot-add into the channel manager behind a trait
//! boundary. Also covers the channel-relay activation path, which shares
//! the "channel hot-add" concept but uses relay infrastructure rather than
//! the WASM runtime.

use super::ActivationFuture;
use crate::extensions::{ActivateResult, ExtensionError};

/// Object-safe port for activating WASM channel extensions (dyn-facing).
///
/// Concrete implementations should implement
/// [`NativeWasmChannelActivationPort`] instead; the blanket adapter boxes
/// futures automatically.
pub trait WasmChannelActivationPort: Send + Sync {
    /// Load, configure, and hot-add the named WASM channel.
    fn activate_wasm_channel<'a>(&'a self, name: &'a str) -> ActivationFuture<'a>;

    /// Activate a channel-relay extension (e.g. Slack via relay service).
    fn activate_channel_relay<'a>(&'a self, name: &'a str) -> ActivationFuture<'a>;
}

/// Native async sibling for WASM channel and channel-relay activation.
pub trait NativeWasmChannelActivationPort: Send + Sync {
    /// Load, configure, and hot-add the named WASM channel.
    fn activate_wasm_channel<'a>(
        &'a self,
        name: &'a str,
    ) -> impl Future<Output = Result<ActivateResult, ExtensionError>> + Send + 'a;

    /// Activate a channel-relay extension.
    fn activate_channel_relay<'a>(
        &'a self,
        name: &'a str,
    ) -> impl Future<Output = Result<ActivateResult, ExtensionError>> + Send + 'a;
}

use std::future::Future;

impl<T> WasmChannelActivationPort for T
where
    T: NativeWasmChannelActivationPort + Send + Sync,
{
    fn activate_wasm_channel<'a>(&'a self, name: &'a str) -> ActivationFuture<'a> {
        Box::pin(NativeWasmChannelActivationPort::activate_wasm_channel(
            self, name,
        ))
    }

    fn activate_channel_relay<'a>(&'a self, name: &'a str) -> ActivationFuture<'a> {
        Box::pin(NativeWasmChannelActivationPort::activate_channel_relay(
            self, name,
        ))
    }
}
