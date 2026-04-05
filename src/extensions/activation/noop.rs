//! No-op activation stubs for test isolation.
//!
//! Each stub returns [`ExtensionError::ActivationFailed`] with a descriptive
//! message, ensuring tests never accidentally trigger real MCP connections,
//! WASM runtime instantiation, or channel hot-adds.

use crate::extensions::ExtensionError;

use super::{
    ActivationFuture, McpActivationPort, WasmChannelActivationPort, WasmToolActivationPort,
};

/// No-op MCP activation stub. Always returns an activation error.
pub struct NoOpMcpActivation;

impl McpActivationPort for NoOpMcpActivation {
    fn activate_mcp<'a>(&'a self, name: &'a str) -> ActivationFuture<'a> {
        Box::pin(async move {
            Err(ExtensionError::ActivationFailed(format!(
                "MCP activation disabled (no-op stub) for '{}'",
                name
            )))
        })
    }
}

impl Default for NoOpMcpActivation {
    fn default() -> Self {
        Self
    }
}

/// No-op WASM tool activation stub. Always returns an activation error.
pub struct NoOpWasmToolActivation;

impl WasmToolActivationPort for NoOpWasmToolActivation {
    fn activate_wasm_tool<'a>(&'a self, name: &'a str) -> ActivationFuture<'a> {
        Box::pin(async move {
            Err(ExtensionError::ActivationFailed(format!(
                "WASM tool activation disabled (no-op stub) for '{}'",
                name
            )))
        })
    }
}

impl Default for NoOpWasmToolActivation {
    fn default() -> Self {
        Self
    }
}

/// No-op WASM channel and channel-relay activation stub.
/// Always returns an activation error.
pub struct NoOpWasmChannelActivation;

impl WasmChannelActivationPort for NoOpWasmChannelActivation {
    fn activate_wasm_channel<'a>(&'a self, name: &'a str) -> ActivationFuture<'a> {
        Box::pin(async move {
            Err(ExtensionError::ActivationFailed(format!(
                "WASM channel activation disabled (no-op stub) for '{}'",
                name
            )))
        })
    }

    fn activate_channel_relay<'a>(&'a self, name: &'a str) -> ActivationFuture<'a> {
        Box::pin(async move {
            Err(ExtensionError::ActivationFailed(format!(
                "Channel relay activation disabled (no-op stub) for '{}'",
                name
            )))
        })
    }
}

impl Default for NoOpWasmChannelActivation {
    fn default() -> Self {
        Self
    }
}
