//! No-op activation stubs for test isolation.
//!
//! Each stub returns [`ExtensionError::ActivationFailed`] with a descriptive
//! message, ensuring tests never accidentally trigger real MCP connections,
//! WASM runtime instantiation, or channel hot-adds.

use crate::extensions::{ActivateResult, ExtensionError};

use super::{
    NativeMcpActivationPort, NativeWasmChannelActivationPort, NativeWasmToolActivationPort,
};

/// No-op MCP activation stub. Always returns an activation error.
pub struct NoOpMcpActivation;

impl NativeMcpActivationPort for NoOpMcpActivation {
    async fn activate_mcp<'a>(&'a self, name: &'a str) -> Result<ActivateResult, ExtensionError> {
        Err(ExtensionError::ActivationFailed(format!(
            "MCP activation disabled (no-op stub) for '{}'",
            name
        )))
    }
}

/// No-op WASM tool activation stub. Always returns an activation error.
pub struct NoOpWasmToolActivation;

impl NativeWasmToolActivationPort for NoOpWasmToolActivation {
    async fn activate_wasm_tool<'a>(
        &'a self,
        name: &'a str,
    ) -> Result<ActivateResult, ExtensionError> {
        Err(ExtensionError::ActivationFailed(format!(
            "WASM tool activation disabled (no-op stub) for '{}'",
            name
        )))
    }
}

/// No-op WASM channel and channel-relay activation stub.
/// Always returns an activation error.
pub struct NoOpWasmChannelActivation;

impl NativeWasmChannelActivationPort for NoOpWasmChannelActivation {
    async fn activate_wasm_channel<'a>(
        &'a self,
        name: &'a str,
    ) -> Result<ActivateResult, ExtensionError> {
        Err(ExtensionError::ActivationFailed(format!(
            "WASM channel activation disabled (no-op stub) for '{}'",
            name
        )))
    }

    async fn activate_channel_relay<'a>(
        &'a self,
        name: &'a str,
    ) -> Result<ActivateResult, ExtensionError> {
        Err(ExtensionError::ActivationFailed(format!(
            "Channel relay activation disabled (no-op stub) for '{}'",
            name
        )))
    }
}
