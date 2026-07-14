//! Implementation of the `mcp list` subcommand.

use crate::tools::mcp::config::EffectiveTransport;

use super::store::{connect_db, load_servers};

/// List configured MCP servers.
pub(super) async fn list_servers(verbose: bool) -> anyhow::Result<()> {
    let db = connect_db().await;
    let servers = load_servers(db.as_deref()).await?;

    if servers.servers.is_empty() {
        println!();
        println!("  No MCP servers configured.");
        println!();
        println!("  Add a server with:");
        println!("    ironclaw mcp add <name> <url> [--client-id <id>]");
        println!();
        return Ok(());
    }

    println!();
    println!("  Configured MCP servers:");
    println!();

    for server in &servers.servers {
        let status = if server.enabled { "●" } else { "○" };
        let auth_status = if server.requires_auth() {
            " (auth required)"
        } else {
            ""
        };

        let effective = server.effective_transport();

        let transport_label = match &effective {
            EffectiveTransport::Http => "http".to_string(),
            EffectiveTransport::Stdio { command, .. } => {
                format!("stdio ({})", command)
            }
            EffectiveTransport::Unix { socket_path } => {
                format!("unix ({})", socket_path)
            }
        };

        if verbose {
            println!("  {} {}{}", status, server.name, auth_status);
            println!("      Transport: {}", transport_label);
            match &effective {
                EffectiveTransport::Http => {
                    println!("      URL: {}", server.url);
                }
                EffectiveTransport::Stdio { command, args, env } => {
                    println!("      Command: {}", command);
                    if !args.is_empty() {
                        println!("      Args: {}", args.join(", "));
                    }
                    if !env.is_empty() {
                        // Only print env var names, not values (may contain secrets).
                        let env_keys: Vec<&str> = env.keys().map(|k| k.as_str()).collect();
                        println!("      Env: {}", env_keys.join(", "));
                    }
                }
                EffectiveTransport::Unix { socket_path } => {
                    println!("      Socket: {}", socket_path);
                }
            }
            if let Some(ref desc) = server.description {
                println!("      Description: {}", desc);
            }
            if let Some(ref oauth) = server.oauth {
                println!("      OAuth Client ID: {}", oauth.client_id);
                if !oauth.scopes.is_empty() {
                    println!("      Scopes: {}", oauth.scopes.join(", "));
                }
            }
            if !server.headers.is_empty() {
                let header_keys: Vec<&String> = server.headers.keys().collect();
                println!(
                    "      Headers: {}",
                    header_keys
                        .iter()
                        .map(|k| k.as_str())
                        .collect::<Vec<_>>()
                        .join(", ")
                );
            }
            println!();
        } else {
            let display = match &effective {
                EffectiveTransport::Http => server.url.clone(),
                EffectiveTransport::Stdio { command, .. } => command.to_string(),
                EffectiveTransport::Unix { socket_path } => socket_path.to_string(),
            };
            println!(
                "  {} {} - {} [{}]{}",
                status, server.name, display, transport_label, auth_status
            );
        }
    }

    if !verbose {
        println!();
        println!("  Use --verbose for more details.");
    }

    println!();

    Ok(())
}
