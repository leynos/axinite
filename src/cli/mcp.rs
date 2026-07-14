//! MCP server management CLI commands.
//!
//! Commands for adding, removing, authenticating, and testing MCP servers.

use std::collections::HashMap;

use crate::tools::mcp::{McpServerConfig, OAuthConfig};

mod command;
mod list;
mod session;
mod store;

pub use command::{McpAddArgs, McpCommand};

use list::list_servers;
use session::{auth_server, test_server};
use store::{connect_db, load_servers, save_servers};

/// Run an MCP command.
pub async fn run_mcp_command(cmd: McpCommand) -> anyhow::Result<()> {
    match cmd {
        McpCommand::Add(args) => add_server(*args).await,
        McpCommand::Remove { name } => remove_server(name).await,
        McpCommand::List { verbose } => list_servers(verbose).await,
        McpCommand::Auth { name, user } => auth_server(name, user).await,
        McpCommand::Test { name, user } => test_server(name, user).await,
        McpCommand::Toggle {
            name,
            enable,
            disable,
        } => toggle_server(name, enable, disable).await,
    }
}

/// Add a new MCP server.
async fn add_server(args: McpAddArgs) -> anyhow::Result<()> {
    let McpAddArgs {
        name,
        url,
        transport,
        command,
        cmd_args,
        env,
        socket,
        headers,
        client_id,
        auth_url,
        token_url,
        scopes,
        description,
    } = args;

    let transport_lower = transport.to_lowercase();

    let mut config = match transport_lower.as_str() {
        "stdio" => {
            let cmd = command
                .clone()
                .ok_or_else(|| anyhow::anyhow!("--command is required for stdio transport"))?;
            let env_map: HashMap<String, String> = env.into_iter().collect();
            McpServerConfig::new_stdio(&name, &cmd, cmd_args.clone(), env_map)
        }
        "unix" => {
            let socket_path = socket
                .clone()
                .ok_or_else(|| anyhow::anyhow!("--socket is required for unix transport"))?;
            McpServerConfig::new_unix(&name, &socket_path)
        }
        "http" => {
            let url_val = url
                .as_deref()
                .ok_or_else(|| anyhow::anyhow!("URL is required for http transport"))?;
            McpServerConfig::new(&name, url_val)
        }
        other => {
            anyhow::bail!(
                "Unknown transport type '{}'. Supported: http, stdio, unix",
                other
            );
        }
    };

    // Apply headers if any
    if !headers.is_empty() {
        let headers_map: HashMap<String, String> = headers.into_iter().collect();
        config = config.with_headers(headers_map);
    }

    if let Some(desc) = description {
        config = config.with_description(desc);
    }

    // Track if auth is required
    let requires_auth = client_id.is_some();

    // Set up OAuth if client_id is provided (HTTP transport only)
    if let Some(client_id) = client_id {
        if transport_lower != "http" {
            anyhow::bail!("OAuth authentication is only supported with http transport");
        }

        let mut oauth = OAuthConfig::new(client_id);

        if let (Some(auth), Some(token)) = (auth_url, token_url) {
            oauth = oauth.with_endpoints(auth, token);
        }

        if let Some(scopes_str) = scopes {
            let scope_list: Vec<String> = scopes_str
                .split(',')
                .map(|s| s.trim().to_string())
                .collect();
            oauth = oauth.with_scopes(scope_list);
        }

        config = config.with_oauth(oauth);
    }

    // Validate
    config.validate()?;

    // Save (DB if available, else disk)
    let db = connect_db().await;
    let mut servers = load_servers(db.as_deref()).await?;
    servers.upsert(config);
    save_servers(db.as_deref(), &servers).await?;

    println!();
    println!("  ✓ Added MCP server '{}'", name);

    match transport_lower.as_str() {
        "stdio" => {
            println!(
                "    Transport: stdio (command: {})",
                command.as_deref().unwrap_or("")
            );
        }
        "unix" => {
            println!(
                "    Transport: unix (socket: {})",
                socket.as_deref().unwrap_or("")
            );
        }
        _ => {
            println!("    URL: {}", url.as_deref().unwrap_or(""));
        }
    }

    if requires_auth {
        println!();
        println!("  Run 'ironclaw mcp auth {}' to authenticate.", name);
    }

    println!();

    Ok(())
}

/// Remove an MCP server.
async fn remove_server(name: String) -> anyhow::Result<()> {
    let db = connect_db().await;
    let mut servers = load_servers(db.as_deref()).await?;
    if !servers.remove(&name) {
        anyhow::bail!("Server '{}' not found", name);
    }
    save_servers(db.as_deref(), &servers).await?;

    println!();
    println!("  ✓ Removed MCP server '{}'", name);
    println!();

    Ok(())
}

/// Toggle server enabled/disabled state.
async fn toggle_server(name: String, enable: bool, disable: bool) -> anyhow::Result<()> {
    let db = connect_db().await;
    let mut servers = load_servers(db.as_deref()).await?;

    let server = servers
        .get_mut(&name)
        .ok_or_else(|| anyhow::anyhow!("Server '{}' not found", name))?;

    let new_state = if enable {
        true
    } else if disable {
        false
    } else {
        !server.enabled // Toggle if neither specified
    };

    server.enabled = new_state;
    save_servers(db.as_deref(), &servers).await?;

    let status = if new_state { "enabled" } else { "disabled" };
    println!();
    println!("  ✓ Server '{}' is now {}.", name, status);
    println!();

    Ok(())
}

#[cfg(test)]
mod tests;
