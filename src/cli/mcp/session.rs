//! Implementations of the `mcp auth` and `mcp test` subcommands.

use std::io::Write;
use std::sync::Arc;

use crate::secrets::SecretsStore;
use crate::tools::mcp::{
    McpClient, McpSessionManager,
    auth::{authorize_mcp_server, is_authenticated},
};

use super::store::{connect_db, load_servers};

/// Load the named server's configuration, failing when it is unknown.
async fn load_server_config(
    name: &str,
) -> anyhow::Result<crate::tools::mcp::config::McpServerConfig> {
    let db = connect_db().await;
    let servers = load_servers(db.as_deref()).await?;
    servers
        .get(name)
        .cloned()
        .ok_or_else(|| anyhow::anyhow!("Server '{}' not found", name))
}

/// Authenticate with an MCP server.
pub(super) async fn auth_server(name: String, user_id: String) -> anyhow::Result<()> {
    let server = load_server_config(&name).await?;

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
    let server = load_server_config(&name).await?;

    println!();
    println!("  Testing connection to '{}'...", name);

    // Always check for stored tokens (from either pre-configured OAuth or DCR)
    let secrets = get_secrets_store().await?;
    let has_tokens = is_authenticated(&server, &secrets, &user_id).await;

    let client = if has_tokens {
        // We have stored tokens, use authenticated client
        let session_manager = Arc::new(McpSessionManager::new());
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
            print_tool_list(&client).await;
        }
        Err(e) => print_connection_failure(&e.to_string(), has_tokens, &name),
    }

    println!();

    Ok(())
}

/// List the server's tools with approval markers and short descriptions.
async fn print_tool_list(client: &McpClient) {
    let tools = match client.list_tools().await {
        Ok(tools) => tools,
        Err(e) => {
            println!("  ✗ Failed to list tools: {}", e);
            return;
        }
    };
    println!("  Available tools ({}):", tools.len());
    for tool in tools {
        let approval = if tool.requires_approval() {
            " [approval required]"
        } else {
            ""
        };
        println!("    • {}{}", tool.name, approval);
        if !tool.description.is_empty() {
            println!("      {}", truncate_description(&tool.description));
        }
    }
}

/// Truncate long tool descriptions to a single display line.
fn truncate_description(description: &str) -> String {
    if description.len() > 60 {
        format!("{}...", &description[..57])
    } else {
        description.to_string()
    }
}

/// Explain a connection failure, distinguishing expired tokens and missing
/// authentication from ordinary connection errors.
fn print_connection_failure(err_str: &str, has_tokens: bool, name: &str) {
    // Check if server requires auth but we don't have valid tokens
    let is_auth_failure = err_str.contains("401") || err_str.contains("requires authentication");
    if !is_auth_failure {
        println!("  ✗ Connection failed: {}", err_str);
        return;
    }
    if has_tokens {
        // We had tokens but they failed - need to re-authenticate
        println!("  ✗ Authentication failed (token may be expired). Try re-authenticating:");
        println!("    ironclaw mcp auth {}", name);
    } else {
        // No tokens - server requires auth
        println!("  ✗ Server requires authentication.");
        println!();
        println!("  Run 'ironclaw mcp auth {}' to authenticate.", name);
    }
}

/// Initialize and return the secrets store.
async fn get_secrets_store() -> anyhow::Result<Arc<dyn SecretsStore + Send + Sync>> {
    crate::cli::init_secrets_store().await
}
