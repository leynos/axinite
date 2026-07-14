//! Persistence helpers shared by the `mcp` subcommands: database
//! connection and MCP server config load/save.

use std::sync::Arc;

use crate::config::Config;
use crate::db::Database;
use crate::tools::mcp::config::{self, McpServersFile};

pub(super) const DEFAULT_USER_ID: &str = "default";

/// Try to connect to the database (backend-agnostic).
pub(super) async fn connect_db() -> Option<Arc<dyn Database>> {
    let config = Config::from_env().await.ok()?;
    crate::db::connect_from_config(&config.database).await.ok()
}

/// Load MCP servers (DB if available, else disk).
pub(super) async fn load_servers(
    db: Option<&dyn Database>,
) -> Result<McpServersFile, config::ConfigError> {
    if let Some(db) = db {
        config::load_mcp_servers_from_db(db, DEFAULT_USER_ID).await
    } else {
        config::load_mcp_servers().await
    }
}

/// Save MCP servers (DB if available, else disk).
pub(super) async fn save_servers(
    db: Option<&dyn Database>,
    servers: &McpServersFile,
) -> Result<(), config::ConfigError> {
    if let Some(db) = db {
        config::save_mcp_servers_to_db(db, DEFAULT_USER_ID, servers).await
    } else {
        config::save_mcp_servers(servers).await
    }
}
