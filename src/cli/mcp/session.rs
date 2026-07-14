//! Implementations of the `mcp auth` and `mcp test` subcommands.

use std::io::Write;
use std::sync::Arc;

use crate::secrets::SecretsStore;
use crate::tools::mcp::{
    McpClient, McpSessionManager,
    auth::{authorize_mcp_server, is_authenticated},
};

use super::store::{connect_db, load_servers};

/// Authenticate with an MCP server.
pub(super) async fn auth_server(name: String, user_id: String) -> anyhow::Result<()> {
    // Get server config
    let db = connect_db().await;
    let servers = load_servers(db.as_deref()).await?;
    let server = servers
        .get(&name)
        .cloned()
        .ok_or_else(|| anyhow::anyhow!("Server '{}' not found", name))?;

    // Initialize secrets store
    let secrets = get_secrets_store().await?;

    // Check if already authenticated
    if is_authenticated(&server, &secrets, &user_id).await {
        println!();
        println!("  Server '{}' is already authenticated.", name);
        println!();
        print!("  Re-authenticate? [y/N]: ");
        std::io::stdout().flush()?;

        let mut input = String::new();
        std::io::stdin().read_line(&mut input)?;

        if !input.trim().eq_ignore_ascii_case("y") {
            return Ok(());
        }
        println!();
    }

    println!();
    println!("╔════════════════════════════════════════════════════════════════╗");
    println!(
        "║  {:^62}║",
        format!("{} Authentication", name.to_uppercase())
    );
    println!("╚════════════════════════════════════════════════════════════════╝");
    println!();

    // Perform OAuth flow (supports both pre-configured OAuth and DCR)
    match authorize_mcp_server(&server, &secrets, &user_id).await {
        Ok(_token) => {
            println!();
            println!("  ✓ Successfully authenticated with '{}'!", name);
            println!();
            println!("  You can now use tools from this server.");
            println!();
        }
        Err(crate::tools::mcp::auth::AuthError::NotSupported) => {
            println!();
            println!("  ✗ Server does not support OAuth authentication.");
            println!();
            println!("  The server may require a different authentication method,");
            println!("  or you may need to configure OAuth manually:");
            println!();
            println!("    ironclaw mcp remove {}", name);
            println!(
                "    ironclaw mcp add {} {} --client-id YOUR_CLIENT_ID",
                name, server.url
            );
            println!();
        }
        Err(e) => {
            println!();
            println!("  ✗ Authentication failed: {}", e);
            println!();
            return Err(e.into());
        }
    }

    Ok(())
}

/// Test connection to an MCP server.
pub(super) async fn test_server(name: String, user_id: String) -> anyhow::Result<()> {
    // Get server config
    let db = connect_db().await;
    let servers = load_servers(db.as_deref()).await?;
    let server = servers
        .get(&name)
        .cloned()
        .ok_or_else(|| anyhow::anyhow!("Server '{}' not found", name))?;

    println!();
    println!("  Testing connection to '{}'...", name);

    // Create client
    let session_manager = Arc::new(McpSessionManager::new());

    // Always check for stored tokens (from either pre-configured OAuth or DCR)
    let secrets = get_secrets_store().await?;
    let has_tokens = is_authenticated(&server, &secrets, &user_id).await;

    let client = if has_tokens {
        // We have stored tokens, use authenticated client
        McpClient::new_authenticated(server.clone(), session_manager, secrets, user_id)
    } else if server.requires_auth() {
        // OAuth configured but no tokens - need to authenticate
        println!();
        println!(
            "  ✗ Not authenticated. Run 'ironclaw mcp auth {}' first.",
            name
        );
        println!();
        return Ok(());
    } else {
        // No OAuth and no tokens - try unauthenticated
        McpClient::new_with_config(server.clone())
    };

    // Test connection
    match client.test_connection().await {
        Ok(()) => {
            println!("  ✓ Connection successful!");
            println!();

            // List tools
            match client.list_tools().await {
                Ok(tools) => {
                    println!("  Available tools ({}):", tools.len());
                    for tool in tools {
                        let approval = if tool.requires_approval() {
                            " [approval required]"
                        } else {
                            ""
                        };
                        println!("    • {}{}", tool.name, approval);
                        if !tool.description.is_empty() {
                            // Truncate long descriptions
                            let desc = if tool.description.len() > 60 {
                                format!("{}...", &tool.description[..57])
                            } else {
                                tool.description.clone()
                            };
                            println!("      {}", desc);
                        }
                    }
                }
                Err(e) => {
                    println!("  ✗ Failed to list tools: {}", e);
                }
            }
        }
        Err(e) => {
            let err_str = e.to_string();
            // Check if server requires auth but we don't have valid tokens
            if err_str.contains("401") || err_str.contains("requires authentication") {
                if has_tokens {
                    // We had tokens but they failed - need to re-authenticate
                    println!(
                        "  ✗ Authentication failed (token may be expired). Try re-authenticating:"
                    );
                    println!("    ironclaw mcp auth {}", name);
                } else {
                    // No tokens - server requires auth
                    println!("  ✗ Server requires authentication.");
                    println!();
                    println!("  Run 'ironclaw mcp auth {}' to authenticate.", name);
                }
            } else {
                println!("  ✗ Connection failed: {}", e);
            }
        }
    }

    println!();

    Ok(())
}

/// Initialize and return the secrets store.
async fn get_secrets_store() -> anyhow::Result<Arc<dyn SecretsStore + Send + Sync>> {
    crate::cli::init_secrets_store().await
}
