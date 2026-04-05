//! WASM channel activation port.
//!
//! Isolates WASM channel loading, credential injection, webhook router
//! registration, and hot-add into the channel manager behind a trait
//! boundary. Also covers the channel-relay activation path, which shares
//! the "channel hot-add" concept but uses relay infrastructure rather than
//! the WASM runtime.

use super::ActivationFuture;

/// Object-safe port for activating WASM channel extensions.
pub trait WasmChannelActivationPort: Send + Sync {
    /// Load, configure, and hot-add the named WASM channel.
    fn activate_wasm_channel<'a>(&'a self, name: &'a str) -> ActivationFuture<'a>;

    /// Activate a channel-relay extension (e.g. Slack via relay service).
    fn activate_channel_relay<'a>(&'a self, name: &'a str) -> ActivationFuture<'a>;
}
