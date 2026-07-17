//! MCP server configuration.
//!
//! Stores configuration for connecting to hosted MCP servers.
//! Configuration is persisted at ~/.axinite/mcp-servers.json.
//!
//! Submodules:
//! - [`server`]: per-server and transport configuration types
//! - [`store`]: file- and database-backed persistence

mod server;
mod store;
#[cfg(test)]
mod tests;

use crate::tools::tool::ToolError;

pub use server::{EffectiveTransport, McpServerConfig, McpTransportConfig, OAuthConfig};
pub use store::{
    McpServersFile, add_mcp_server, add_mcp_server_db, default_config_path, get_mcp_server,
    load_mcp_servers, load_mcp_servers_from, load_mcp_servers_from_db, remove_mcp_server,
    remove_mcp_server_db, save_mcp_servers, save_mcp_servers_to, save_mcp_servers_to_db,
};

/// Error type for MCP configuration operations.
#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("Invalid configuration: {reason}")]
    InvalidConfig { reason: String },

    #[error("Server not found: {name}")]
    ServerNotFound { name: String },
}

impl From<ConfigError> for ToolError {
    fn from(err: ConfigError) -> Self {
        ToolError::ExternalService(err.to_string())
    }
}
