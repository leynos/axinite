//! Tunnel provider selection and the ngrok, Tailscale, custom, and
//! static-URL setup flows.

use secrecy::ExposeSecret;

use crate::settings::{Settings, TunnelSettings};
use crate::setup::prompts::{
    confirm, input, optional_input, print_error, print_info, print_success, secret_input,
    select_one,
};

use super::cloudflare::setup_tunnel_cloudflare;
use super::secrets::ChannelSetupError;

/// Set up a tunnel for exposing the agent to the internet.
///
/// This is shared across all channels that need webhook endpoints.
/// Returns a `TunnelSettings` with provider config (managed tunnel)
/// or a static URL.
pub async fn setup_tunnel(settings: &Settings) -> Result<TunnelSettings, ChannelSetupError> {
    let has_existing = settings.tunnel.public_url.is_some() || settings.tunnel.provider.is_some();
    if has_existing {
        print_existing_tunnel_config(&settings.tunnel);
        if !confirm("Change tunnel configuration?", false)? {
            return Ok(settings.tunnel.clone());
        }
    }

    println!();
    print_info("Tunnel Configuration (for webhook endpoints):");
    print_info("A tunnel exposes your local agent to the internet, enabling:");
    print_info("  - Instant Telegram message delivery (instead of polling)");
    print_info("  - Slack, Discord, GitHub webhooks");
    println!();

    if !confirm("Configure a tunnel?", false)? {
        return Ok(TunnelSettings::default());
    }

    let options = &[
        "ngrok         - managed tunnel, starts automatically",
        "Cloudflare    - cloudflared tunnel, starts automatically",
        "Tailscale     - Tailscale Funnel/Serve, starts automatically",
        "Custom        - your own tunnel command",
        "Static URL    - you manage the tunnel yourself",
    ];

    let choice = select_one("Select tunnel provider:", options)?;

    match choice {
        0 => setup_tunnel_ngrok(),
        1 => setup_tunnel_cloudflare().await,
        2 => setup_tunnel_tailscale(),
        3 => setup_tunnel_custom(),
        4 => setup_tunnel_static(),
        _ => Ok(TunnelSettings::default()),
    }
}

/// Print a summary of the currently configured tunnel.
fn print_existing_tunnel_config(t: &TunnelSettings) {
    println!();
    print_info("Current tunnel configuration:");
    print_existing_provider_details(t);
    if let Some(ref url) = t.public_url {
        print_info(&format!("  URL:       {}", url));
    }
    println!();
}

/// Print the provider-specific lines of the current tunnel configuration.
fn print_existing_provider_details(t: &TunnelSettings) {
    match t.provider.as_deref() {
        Some("ngrok") => print_ngrok_details(t),
        Some("cloudflare") => print_cloudflare_details(t),
        Some("tailscale") => print_tailscale_details(t),
        Some("custom") => print_custom_details(t),
        Some(other) => {
            print_info(&format!("  Provider:  {}", other));
        }
        None => {}
    }
}

/// Print the ngrok-specific configuration lines.
fn print_ngrok_details(t: &TunnelSettings) {
    print_info("  Provider:  ngrok");
    if let Some(ref domain) = t.ngrok_domain {
        print_info(&format!("  Domain:    {}", domain));
    }
    if t.ngrok_token.is_some() {
        print_info("  Auth:      token configured");
    }
}

/// Print the Cloudflare-specific configuration lines.
fn print_cloudflare_details(t: &TunnelSettings) {
    print_info("  Provider:  Cloudflare Tunnel");
    if t.cf_token.is_some() {
        print_info("  Auth:      token configured");
    }
}

/// Print the Tailscale-specific configuration lines.
fn print_tailscale_details(t: &TunnelSettings) {
    let mode = if t.ts_funnel {
        "Funnel (public)"
    } else {
        "Serve (tailnet-only)"
    };
    print_info(&format!("  Provider:  Tailscale {}", mode));
    if let Some(ref hostname) = t.ts_hostname {
        print_info(&format!("  Hostname:  {}", hostname));
    }
}

/// Print the custom-command configuration lines.
fn print_custom_details(t: &TunnelSettings) {
    print_info("  Provider:  Custom command");
    if let Some(ref cmd) = t.custom_command {
        print_info(&format!("  Command:   {}", cmd));
    }
    if let Some(ref url) = t.custom_health_url {
        print_info(&format!("  Health:    {}", url));
    }
}

fn setup_tunnel_ngrok() -> Result<TunnelSettings, ChannelSetupError> {
    print_info("Get your auth token from: https://dashboard.ngrok.com/get-started/your-authtoken");
    println!();

    let token = secret_input("ngrok auth token")?;
    let domain = optional_input("Custom domain", Some("leave empty for auto-assigned"))?;

    print_success("ngrok configured. Tunnel will start automatically at boot.");

    Ok(TunnelSettings {
        provider: Some("ngrok".to_string()),
        ngrok_token: Some(token.expose_secret().to_string()),
        ngrok_domain: domain,
        ..Default::default()
    })
}

fn setup_tunnel_tailscale() -> Result<TunnelSettings, ChannelSetupError> {
    let funnel = confirm("Use Tailscale Funnel (public internet)?", true)?;
    let hostname = optional_input("Hostname override", Some("leave empty for auto-detect"))?;

    let mode = if funnel {
        "Funnel (public)"
    } else {
        "Serve (tailnet-only)"
    };
    print_success(&format!("Tailscale {} configured.", mode));

    Ok(TunnelSettings {
        provider: Some("tailscale".to_string()),
        ts_funnel: funnel,
        ts_hostname: hostname,
        ..Default::default()
    })
}

fn setup_tunnel_custom() -> Result<TunnelSettings, ChannelSetupError> {
    print_info("Enter a shell command to start your tunnel.");
    print_info("Use {port} and {host} as placeholders.");
    print_info("Example: bore local {port} --to bore.pub");
    println!();

    let command = input("Tunnel command")?;
    if command.is_empty() {
        return Err(ChannelSetupError::Validation(
            "Tunnel command cannot be empty".to_string(),
        ));
    }

    let health_url = optional_input("Health check URL", Some("optional"))?;
    let url_pattern = optional_input(
        "URL pattern (substring to match in stdout)",
        Some("optional"),
    )?;

    print_success("Custom tunnel configured.");

    Ok(TunnelSettings {
        provider: Some("custom".to_string()),
        custom_command: Some(command),
        custom_health_url: health_url,
        custom_url_pattern: url_pattern,
        ..Default::default()
    })
}

fn setup_tunnel_static() -> Result<TunnelSettings, ChannelSetupError> {
    print_info("Enter the public URL of your externally managed tunnel.");
    println!();

    let tunnel_url = input("Tunnel URL (e.g., https://abc123.ngrok.io)")?;

    if !tunnel_url.starts_with("https://") {
        print_error("URL must start with https:// (webhooks require HTTPS)");
        return Err(ChannelSetupError::Validation(
            "Invalid tunnel URL: must use HTTPS".to_string(),
        ));
    }

    let tunnel_url = tunnel_url.trim_end_matches('/').to_string();

    print_success(&format!("Static tunnel URL configured: {}", tunnel_url));
    print_info("Make sure your tunnel is running before starting the agent.");

    Ok(TunnelSettings {
        public_url: Some(tunnel_url),
        ..Default::default()
    })
}
