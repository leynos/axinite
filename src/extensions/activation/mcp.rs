//! MCP server activation port.
//!
//! Isolates the concrete MCP client lifecycle (session creation, tool
//! listing, tool registration) behind a trait boundary so that
//! [`ExtensionManager`](super::super::ExtensionManager) need not depend
//! on MCP transport details.

use super::ActivationFuture;

/// Object-safe port for activating MCP server extensions.
pub trait McpActivationPort: Send + Sync {
    /// Connect to the named MCP server, list its tools, register them,
    /// and return the activation result.
    fn activate_mcp<'a>(&'a self, name: &'a str) -> ActivationFuture<'a>;
}
