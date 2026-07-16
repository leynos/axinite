//! CLI argument types and value parsers for the `mcp` subcommands.

use clap::{Args, Subcommand};

/// Arguments for the `mcp add` subcommand.
#[derive(Args, Debug, Clone)]
pub struct McpAddArgs {
    /// Server name (e.g., "notion", "github")
    pub name: String,

    /// Server URL (e.g., "https://mcp.notion.com") -- required for http transport
    pub url: Option<String>,

    /// Transport type: http (default), stdio, unix
    #[arg(long, default_value = "http")]
    pub transport: String,

    /// Command to run (stdio transport)
    #[arg(long)]
    pub command: Option<String>,

    /// Command arguments (stdio transport, can be repeated)
    #[arg(long = "arg", num_args = 1..)]
    pub cmd_args: Vec<String>,

    /// Environment variables (stdio transport, KEY=VALUE format, can be repeated)
    #[arg(long = "env", value_parser = parse_env_var)]
    pub env: Vec<(String, String)>,

    /// Unix socket path (unix transport)
    #[arg(long)]
    pub socket: Option<String>,

    /// Custom HTTP headers (KEY:VALUE format, can be repeated)
    #[arg(long = "header", value_parser = parse_header)]
    pub headers: Vec<(String, String)>,

    /// OAuth client ID (if authentication is required)
    #[arg(long)]
    pub client_id: Option<String>,

    /// OAuth authorization URL (optional, can be discovered)
    #[arg(long)]
    pub auth_url: Option<String>,

    /// OAuth token URL (optional, can be discovered)
    #[arg(long)]
    pub token_url: Option<String>,

    /// Scopes to request (comma-separated)
    #[arg(long)]
    pub scopes: Option<String>,

    /// Server description
    #[arg(long)]
    pub description: Option<String>,
}

#[derive(Subcommand, Debug, Clone)]
pub enum McpCommand {
    /// Add an MCP server
    Add(Box<McpAddArgs>),

    /// Remove an MCP server
    Remove {
        /// Server name to remove
        name: String,
    },

    /// List configured MCP servers
    List {
        /// Show detailed information
        #[arg(short, long)]
        verbose: bool,
    },

    /// Authenticate with an MCP server (OAuth flow)
    Auth {
        /// Server name to authenticate
        name: String,

        /// User ID for storing the token (default: "default")
        #[arg(short, long, default_value = "default")]
        user: String,
    },

    /// Test connection to an MCP server
    Test {
        /// Server name to test
        name: String,

        /// User ID for authentication (default: "default")
        #[arg(short, long, default_value = "default")]
        user: String,
    },

    /// Enable or disable an MCP server
    Toggle {
        /// Server name
        name: String,

        /// Enable the server
        #[arg(long, conflicts_with = "disable")]
        enable: bool,

        /// Disable the server
        #[arg(long, conflicts_with = "enable")]
        disable: bool,
    },
}

pub(super) fn parse_header(s: &str) -> Result<(String, String), String> {
    let pos = s
        .find(':')
        .ok_or_else(|| format!("invalid header format '{}', expected KEY:VALUE", s))?;
    Ok((s[..pos].trim().to_string(), s[pos + 1..].trim().to_string()))
}

pub(super) fn parse_env_var(s: &str) -> Result<(String, String), String> {
    let pos = s
        .find('=')
        .ok_or_else(|| format!("invalid env var format '{}', expected KEY=VALUE", s))?;
    Ok((s[..pos].to_string(), s[pos + 1..].to_string()))
}
