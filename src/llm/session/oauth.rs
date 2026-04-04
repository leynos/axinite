//! Interactive NEAR AI authentication helpers.
//!
//! This module coordinates browser-based OAuth via `oauth_helpers` and the
//! callback listener on `OAUTH_CALLBACK_PORT`, plus the API-key login path used
//! by `SessionManager` renewal flows.

use secrecy::{ExposeSecret, SecretString};

use super::{LlmError, SessionManager};
use crate::llm::oauth_helpers;
use crate::llm::oauth_helpers::OAUTH_CALLBACK_PORT;

fn read_auth_choice() -> Result<String, LlmError> {
    println!();
    println!("╔════════════════════════════════════════════════════════════════╗");
    println!("║                    NEAR AI Authentication                      ║");
    println!("╠════════════════════════════════════════════════════════════════╣");
    println!("║  Choose an authentication method:                              ║");
    println!("║                                                                ║");
    println!("║    [1] GitHub            (requires localhost browser access)   ║");
    println!("║    [2] Google            (requires localhost browser access)   ║");
    println!("║    [3] NEAR Wallet (coming soon)                               ║");
    println!("║    [4] NEAR AI Cloud API key                                   ║");
    println!("║                                                                ║");
    println!("╚════════════════════════════════════════════════════════════════╝");
    println!();
    print!("Enter choice [1-4]: ");

    use std::io::Write;
    std::io::stdout()
        .flush()
        .map_err(|error| LlmError::SessionRenewalFailed {
            provider: "nearai".to_string(),
            reason: format!("Failed to flush prompt: {error}"),
        })?;

    let mut choice = String::new();
    std::io::stdin()
        .read_line(&mut choice)
        .map_err(|e| LlmError::SessionRenewalFailed {
            provider: "nearai".to_string(),
            reason: format!("Failed to read input: {}", e),
        })?;

    Ok(choice)
}

fn warn_if_remote_host(host: &str) {
    if !oauth_helpers::is_loopback_host(host) {
        println!();
        println!("Warning: OAuth callback is using plain HTTP to a remote host ({host}).");
        println!("         The session token will be transmitted unencrypted.");
        println!("         Consider SSH port forwarding instead:");
        println!(
            "           ssh -L {OAUTH_CALLBACK_PORT}:127.0.0.1:{OAUTH_CALLBACK_PORT} user@{host}"
        );
    }
}

fn build_auth_url(choice: &str, auth_base_url: &str, cb_url: &str) -> (&'static str, String) {
    match choice.trim() {
        "2" => {
            let url = format!(
                "{}/v1/auth/google?frontend_callback={}",
                auth_base_url,
                urlencoding::encode(cb_url)
            );
            ("google", url)
        }
        _ => {
            let url = format!(
                "{}/v1/auth/github?frontend_callback={}",
                auth_base_url,
                urlencoding::encode(cb_url)
            );
            ("github", url)
        }
    }
}

async fn complete_browser_oauth(
    manager: &SessionManager,
    listener: tokio::net::TcpListener,
    auth_provider: &str,
    auth_url: &str,
) -> Result<(), LlmError> {
    println!();
    println!("Opening {} authentication...", auth_provider);
    println!();
    println!("  {}", auth_url);
    println!();

    if let Err(error) = open::that(auth_url) {
        tracing::debug!("Could not open browser automatically: {}", error);
        println!("(Could not open browser automatically, please copy the URL above)");
    } else {
        println!("(Opening browser...)");
    }
    println!();
    println!("Waiting for authentication...");

    let session_token =
        oauth_helpers::wait_for_callback(listener, "/auth/callback", "token", "NEAR AI", None)
            .await
            .map_err(|e| LlmError::SessionRenewalFailed {
                provider: "nearai".to_string(),
                reason: e.to_string(),
            })?;

    manager
        .save_session(&session_token, Some(auth_provider))
        .await?;

    {
        let mut guard = manager.token.write().await;
        *guard = Some(SecretString::from(session_token));
    }

    println!();
    println!("✓ Authentication successful!");
    println!();

    Ok(())
}

pub(super) async fn initiate_login_flow(manager: &SessionManager) -> Result<(), LlmError> {
    let cb_url = oauth_helpers::callback_url();
    let host = oauth_helpers::callback_host();
    let choice = tokio::task::spawn_blocking(read_auth_choice)
        .await
        .map_err(|error| LlmError::SessionRenewalFailed {
            provider: "nearai".to_string(),
            reason: format!("Authentication prompt task failed: {error}"),
        })??;

    match choice.trim() {
        "4" => return manager.api_key_login().await,
        "3" => {
            println!();
            println!("NEAR Wallet authentication is not yet implemented.");
            println!("Please use GitHub or Google for now.");
            return Err(LlmError::SessionRenewalFailed {
                provider: "nearai".to_string(),
                reason: "NEAR Wallet auth not yet implemented".to_string(),
            });
        }
        "1" | "" | "2" => {}
        other => {
            return Err(LlmError::SessionRenewalFailed {
                provider: "nearai".to_string(),
                reason: format!("Invalid choice: {}", other),
            });
        }
    }

    warn_if_remote_host(&host);

    let listener = oauth_helpers::bind_callback_listener().await.map_err(|e| {
        LlmError::SessionRenewalFailed {
            provider: "nearai".to_string(),
            reason: e.to_string(),
        }
    })?;

    let (auth_provider, auth_url) =
        build_auth_url(choice.trim(), &manager.config.auth_base_url, &cb_url);

    complete_browser_oauth(manager, listener, auth_provider, &auth_url).await
}

pub(super) async fn api_key_flow(_manager: &SessionManager) -> Result<(), LlmError> {
    println!();
    println!("NEAR AI Cloud API key");
    println!("─────────────────────");
    println!();
    println!("  1. Open https://cloud.near.ai in your browser");
    println!("  2. Sign in and navigate to API Keys");
    println!("  3. Create or copy an existing API key");
    println!();

    let key_secret = tokio::task::spawn_blocking(|| crate::setup::secret_input("API key"))
        .await
        .map_err(|error| LlmError::SessionRenewalFailed {
            provider: "nearai".to_string(),
            reason: format!("API key prompt task failed: {error}"),
        })?
        .map_err(|e| LlmError::SessionRenewalFailed {
            provider: "nearai".to_string(),
            reason: format!("Failed to read input: {}", e),
        })?;

    let key = key_secret.expose_secret().trim().to_string();
    if key.is_empty() {
        return Err(LlmError::SessionRenewalFailed {
            provider: "nearai".to_string(),
            reason: "API key cannot be empty".to_string(),
        });
    }

    _manager.set_api_key(SecretString::from(key.clone())).await;

    println!();
    crate::setup::print_success("NEAR AI Cloud API key saved.");
    println!();

    Ok(())
}
