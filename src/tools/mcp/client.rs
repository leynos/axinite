//! MCP client for connecting to MCP servers.
//!
//! Supports both local (unauthenticated) and hosted (OAuth-authenticated) servers.
//! Uses pluggable transports (HTTP, stdio, Unix) via the `McpTransport` trait.
//!
//! ## Module layout
//!
//! - [`core`] — `McpClient` construction, accessors, and cloning
//! - [`requests`] — request sending, auth, and protocol operations
//! - [`wrapper`] — the `Tool` wrapper for remote MCP tools

mod core;
mod requests;
mod wrapper;

#[cfg(test)]
mod tests;

pub use self::core::{McpClient, TransportClientOptions};
#[cfg(test)]
pub(super) use wrapper::McpToolWrapper;
