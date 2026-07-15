//! Cloudflare Tunnel setup flow, conflict detection, and token validation.
//!
//! Command names, command arguments, and `cloudflared` log lines are handled
//! as plain strings throughout: they are free-form external text with no
//! domain invariants a newtype could enforce. Tunnel tokens are carried as
//! [`secrecy::SecretString`] and only exposed at the point of use.

use base64::Engine;
use secrecy::ExposeSecret;

use crate::settings::TunnelSettings;
use crate::setup::prompts::{
    confirm, print_error, print_info, print_success, print_warning, secret_input,
};

use super::secrets::ChannelSetupError;

pub(super) async fn setup_tunnel_cloudflare() -> Result<TunnelSettings, ChannelSetupError> {
    let cloudflared_found = ensure_cloudflared_installed()?;
    confirm_no_conflicting_services()?;

    print_info("Get your tunnel token from the Cloudflare Zero Trust dashboard:");
    print_info("  https://one.dash.cloudflare.com/ > Networks > Tunnels");
    println!();

    let token = secret_input("Cloudflare tunnel token")?;
    let token_valid = confirm_token_format(&token)?;

    // Live-validate the token by briefly spawning cloudflared (if available)
    if cloudflared_found && token_valid {
        confirm_token_live(&token).await?;
    }

    print_completion_notes(cloudflared_found);

    Ok(TunnelSettings {
        provider: Some("cloudflare".to_string()),
        cf_token: Some(token.expose_secret().to_string()),
        ..Default::default()
    })
}

/// Check that the `cloudflared` binary is on `PATH`, offering to continue
/// without it. Returns whether the binary was found.
fn ensure_cloudflared_installed() -> Result<bool, ChannelSetupError> {
    let cloudflared_found = crate::skills::gating::binary_exists("cloudflared");
    if cloudflared_found {
        return Ok(true);
    }

    print_error("cloudflared not found in PATH.");
    print_info("Install it:");
    print_info("  macOS:   brew install cloudflared");
    print_info("  Ubuntu:  https://pkg.cloudflare.com/");
    print_info(
        "  Other:   https://developers.cloudflare.com/cloudflare-one/connections/connect-networks/downloads/",
    );
    println!();
    if !confirm(
        "Continue anyway (you can install cloudflared later)?",
        false,
    )? {
        return Err(ChannelSetupError::Validation(
            "cloudflared binary not found. Install it and re-run setup.".to_string(),
        ));
    }
    Ok(false)
}

/// Warn about existing cloudflared services and let the user abort setup.
fn confirm_no_conflicting_services() -> Result<(), ChannelSetupError> {
    let Some(warning) = detect_existing_cloudflared() else {
        return Ok(());
    };
    print_warning(&warning);
    if !confirm("Continue anyway?", true)? {
        return Err(ChannelSetupError::Cancelled);
    }
    println!();
    Ok(())
}

/// Check the token's format, offering to save an unrecognized token anyway.
/// Returns whether the token matched the expected format.
fn confirm_token_format(token: &secrecy::SecretString) -> Result<bool, ChannelSetupError> {
    if validate_cloudflare_token_format(token.expose_secret()) {
        return Ok(true);
    }

    print_error("Token does not appear to be a valid Cloudflare tunnel token.");
    print_info("Tokens are base64-encoded and contain account/tunnel identifiers.");
    print_info("Copy the full token from: Zero Trust dashboard > Networks > Tunnels > your tunnel");
    println!();
    if !confirm("Save this token anyway?", false)? {
        return Err(ChannelSetupError::Validation(
            "Invalid Cloudflare tunnel token format.".to_string(),
        ));
    }
    Ok(false)
}

/// Verify the token by briefly running cloudflared, offering to save a
/// rejected token anyway.
async fn confirm_token_live(token: &secrecy::SecretString) -> Result<(), ChannelSetupError> {
    print_info("Verifying token with cloudflared...");
    match validate_cloudflare_token_live(token).await {
        Ok(()) => {
            print_success("Token verified -- cloudflared connected successfully.");
            Ok(())
        }
        Err(stderr_output) => {
            print_error(&format!(
                "cloudflared rejected the token: {}",
                stderr_output
            ));
            println!();
            if !confirm("Save this token anyway?", false)? {
                return Err(ChannelSetupError::Validation(
                    "Cloudflare tunnel token failed live validation.".to_string(),
                ));
            }
            Ok(())
        }
    }
}

/// Print the final success message and instructions for starting the tunnel.
fn print_completion_notes(cloudflared_found: bool) {
    print_success("Cloudflare tunnel token saved.");
    if cloudflared_found {
        print_info("Start the tunnel with: cloudflared tunnel --no-autoupdate run --token <token>");
        print_info("For auto-start, install cloudflared as a system service:");
        print_info("  sudo cloudflared service install <token>");
    } else {
        print_info("After installing cloudflared, start the tunnel with:");
        print_info("  cloudflared tunnel --no-autoupdate run --token <token>");
    }
}

/// Detect running cloudflared processes or managed services that could conflict
/// with IronClaw's tunnel management.
fn detect_existing_cloudflared() -> Option<String> {
    #[allow(unused_mut)]
    let mut conflicts: Vec<String> = Vec::new();

    #[cfg(unix)]
    detect_running_cloudflared_processes(&mut conflicts);

    #[cfg(target_os = "macos")]
    detect_macos_cloudflared_services(&mut conflicts);

    #[cfg(target_os = "linux")]
    detect_linux_cloudflared_service(&mut conflicts);

    if conflicts.is_empty() {
        None
    } else {
        Some(format!(
            "Detected existing cloudflared service(s) that may conflict:\n  {}\n\
             Consider stopping them first (e.g., `brew services stop cloudflared` or \
             `sudo systemctl stop cloudflared`).",
            conflicts.join("\n  ")
        ))
    }
}

/// Run a command with stderr suppressed, returning its output when it
/// executed successfully (regardless of exit status).
#[cfg(unix)]
fn capture_command_output(program: &str, args: &[&str]) -> Option<std::process::Output> {
    std::process::Command::new(program)
        .args(args)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .output()
        .ok()
}

/// Record any running cloudflared processes reported by `pgrep`.
#[cfg(unix)]
fn detect_running_cloudflared_processes(conflicts: &mut Vec<String>) {
    let Some(out) = capture_command_output("pgrep", &["-x", "cloudflared"]) else {
        return;
    };
    if !out.status.success() {
        return;
    }
    let pids = String::from_utf8_lossy(&out.stdout);
    let pids: Vec<&str> = pids.trim().lines().collect();
    if !pids.is_empty() {
        conflicts.push(format!(
            "Running cloudflared process(es): PID {}",
            pids.join(", ")
        ));
    }
}

/// Record cloudflared services managed by Homebrew or launchd on macOS.
#[cfg(target_os = "macos")]
fn detect_macos_cloudflared_services(conflicts: &mut Vec<String>) {
    if brew_reports_cloudflared_started() {
        conflicts.push("Homebrew service: cloudflared (started)".to_string());
    }
    if launchd_lists_cloudflared() {
        conflicts.push("launchd service: cloudflared detected".to_string());
    }
}

/// Return `true` when `brew services list` reports a started cloudflared
/// service.
#[cfg(target_os = "macos")]
fn brew_reports_cloudflared_started() -> bool {
    let Some(out) = capture_command_output("brew", &["services", "list"]) else {
        return false;
    };
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .any(|line| line.contains("cloudflared") && line.contains("started"))
}

/// Return `true` when `launchctl list` mentions a cloudflared service.
#[cfg(target_os = "macos")]
fn launchd_lists_cloudflared() -> bool {
    let Some(out) = capture_command_output("launchctl", &["list"]) else {
        return false;
    };
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .any(|line| line.contains("cloudflared"))
}

/// Record an active cloudflared systemd service on Linux.
#[cfg(target_os = "linux")]
fn detect_linux_cloudflared_service(conflicts: &mut Vec<String>) {
    if let Some(out) = capture_command_output("systemctl", &["is-active", "cloudflared"]) {
        let stdout = String::from_utf8_lossy(&out.stdout);
        if stdout.trim() == "active" {
            conflicts.push("systemd service: cloudflared (active)".to_string());
        }
    }
}

/// Validate a Cloudflare tunnel token by briefly running `cloudflared`.
///
/// Spawns `cloudflared tunnel run` with a dummy local URL and watches stderr
/// for up to 10 seconds. If a connection URL appears, the token is valid.
/// If error indicators appear first, returns the error message.
async fn validate_cloudflare_token_live(token: &secrecy::SecretString) -> Result<(), String> {
    use tokio::io::AsyncBufReadExt;
    use tokio::process::Command;

    let mut child = Command::new("cloudflared")
        .args([
            "tunnel",
            "--no-autoupdate",
            "run",
            "--token",
            token.expose_secret(),
            "--url",
            "http://localhost:1",
        ])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .map_err(|e| format!("Failed to spawn cloudflared: {}", e))?;

    let stderr = child
        .stderr
        .take()
        .ok_or_else(|| "Failed to capture cloudflared stderr".to_string())?;
    let mut reader = tokio::io::BufReader::new(stderr).lines();

    let result = tokio::time::timeout(std::time::Duration::from_secs(10), async {
        while let Ok(Some(line)) = reader.next_line().await {
            // A successful connection logs a URL like "https://xxx.cfargotunnel.com"
            if is_tunnel_connected_line(&line) {
                return Ok(());
            }
            // Error indicators that appear before a URL mean the token is bad
            if is_tunnel_error_line(&line.to_lowercase()) {
                return Err(line);
            }
        }
        // Process exited without clear signal -- check exit status
        Err("cloudflared exited without establishing a connection".to_string())
    })
    .await;

    // Ensure the process is killed regardless of outcome
    let _ = child.kill().await;

    match result {
        Ok(inner) => inner,
        Err(_elapsed) => {
            // Timed out without error or success -- benefit of the doubt
            Ok(())
        }
    }
}

/// Return `true` when a `cloudflared` stderr line reports a tunnel URL,
/// which signals a successful connection.
fn is_tunnel_connected_line(line: &str) -> bool {
    let has_tunnel_domain = line.contains("cfargotunnel.com") || line.contains("trycloudflare.com");
    line.contains("https://") && has_tunnel_domain
}

/// Return `true` when a lowercased `cloudflared` stderr line carries an
/// error indicator that means the token is bad.
fn is_tunnel_error_line(lower: &str) -> bool {
    if lower.starts_with("err") {
        return true;
    }
    lower.contains("failed to unmarshal") || lower.contains("unauthorized")
}

/// Validate that a Cloudflare tunnel token has the expected format.
///
/// Cloudflare tunnel tokens are base64-encoded JSON objects containing
/// at least `"a"` (account tag) and `"t"` (tunnel ID) fields.
pub(super) fn validate_cloudflare_token_format(token: &str) -> bool {
    base64::engine::general_purpose::STANDARD
        .decode(token)
        .or_else(|_| base64::engine::general_purpose::URL_SAFE.decode(token))
        .ok()
        .and_then(|bytes| serde_json::from_slice::<serde_json::Value>(&bytes).ok())
        .is_some_and(|json| json.get("a").is_some() && json.get("t").is_some())
}
