//! Implementation of the `mcp list` subcommand.

use crate::tools::mcp::config::{EffectiveTransport, McpServerConfig};

use super::store::{connect_db, load_servers};

/// Print the guidance shown when no servers are configured.
fn print_no_servers_help() {
    println!();
    println!("  No MCP servers configured.");
    println!();
    println!("  Add a server with:");
    println!("    axinite mcp add <name> <url> [--client-id <id>]");
    println!();
}

/// Enabled/disabled status marker for a server.
fn status_marker(server: &McpServerConfig) -> &'static str {
    if server.enabled { "●" } else { "○" }
}

/// Suffix noting that a server requires authentication.
fn auth_suffix(server: &McpServerConfig) -> &'static str {
    if server.requires_auth() {
        " (auth required)"
    } else {
        ""
    }
}

/// Human-readable transport label (with command/socket detail).
fn transport_label(effective: &EffectiveTransport) -> String {
    match effective {
        EffectiveTransport::Http => "http".to_string(),
        EffectiveTransport::Stdio { command, .. } => format!("stdio ({})", command),
        EffectiveTransport::Unix { socket_path } => format!("unix ({})", socket_path),
    }
}

/// Print the transport-specific detail lines in verbose mode.
fn print_transport_details(server: &McpServerConfig, effective: &EffectiveTransport) {
    match effective {
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
}

/// Print one server with full details.
fn print_server_verbose(server: &McpServerConfig) {
    let effective = server.effective_transport();
    println!(
        "  {} {}{}",
        status_marker(server),
        server.name,
        auth_suffix(server)
    );
    println!("      Transport: {}", transport_label(&effective));
    print_transport_details(server, &effective);
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
}

/// Print one server as a single summary line.
fn print_server_brief(server: &McpServerConfig) {
    let effective = server.effective_transport();
    let display = match &effective {
        EffectiveTransport::Http => server.url.clone(),
        EffectiveTransport::Stdio { command, .. } => command.to_string(),
        EffectiveTransport::Unix { socket_path } => socket_path.to_string(),
    };
    println!(
        "  {} {} - {} [{}]{}",
        status_marker(server),
        server.name,
        display,
        transport_label(&effective),
        auth_suffix(server)
    );
}

/// List configured MCP servers.
pub(super) async fn list_servers(verbose: bool) -> anyhow::Result<()> {
    let db = connect_db().await;
    let servers = load_servers(db.as_deref()).await?;

    if servers.servers.is_empty() {
        print_no_servers_help();
        return Ok(());
    }

    println!();
    println!("  Configured MCP servers:");
    println!();

    for server in &servers.servers {
        if verbose {
            print_server_verbose(server);
        } else {
            print_server_brief(server);
        }
    }

    if !verbose {
        println!();
        println!("  Use --verbose for more details.");
    }

    println!();

    Ok(())
}
