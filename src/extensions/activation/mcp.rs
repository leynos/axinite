//! MCP server activation port.
//!
//! Isolates the concrete MCP client lifecycle (session creation, tool
//! listing, tool registration) behind a trait boundary so that
//! [`ExtensionManager`](super::super::ExtensionManager) need not depend
//! on MCP transport details.

use super::ActivationFuture;
use crate::extensions::{ActivateResult, ExtensionError};

/// Object-safe port for activating MCP server extensions (dyn-facing).
///
/// Concrete implementations should implement [`NativeMcpActivationPort`]
/// instead; the blanket adapter boxes futures automatically.
pub trait McpActivationPort: Send + Sync {
    /// Connect to the named MCP server, list its tools, register them,
    /// and return the activation result.
    fn activate_mcp<'a>(&'a self, name: &'a str) -> ActivationFuture<'a>;
}

/// Native async sibling for MCP server activation.
pub trait NativeMcpActivationPort: Send + Sync {
    /// Connect to the named MCP server and register its tools.
    fn activate_mcp<'a>(
        &'a self,
        name: &'a str,
    ) -> impl Future<Output = Result<ActivateResult, ExtensionError>> + Send + 'a;
}

use std::future::Future;

impl<T> McpActivationPort for T
where
    T: NativeMcpActivationPort + Send + Sync,
{
    fn activate_mcp<'a>(&'a self, name: &'a str) -> ActivationFuture<'a> {
        Box::pin(NativeMcpActivationPort::activate_mcp(self, name))
    }
}
