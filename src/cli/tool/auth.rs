//! Authentication flows for installed WASM tools.
//!
//! This module handles environment-backed, manual, and OAuth-driven tool
//! credential setup while delegating the browser-based OAuth mechanics to a
//! focused submodule.

use std::io::Write;
use std::path::PathBuf;

use tokio::fs;

use crate::secrets::SecretsStore;
use crate::tools::wasm::{AuthCapabilitySchema, CapabilitiesFile, ValidationEndpointSchema};

use super::default_tools_dir;
use super::init_secrets_store;
use super::printing::validate_tool_name;

mod oauth;

use self::oauth::{auth_tool_oauth, combine_provider_scopes};

/// Configure authentication for a tool.
pub(super) async fn auth_tool(
    name: String,
    dir: Option<PathBuf>,
    user_id: String,
) -> anyhow::Result<()> {
    validate_tool_name(&name)?;
    let tools_dir = dir.unwrap_or_else(default_tools_dir);
    let caps_path = tools_dir.join(format!("{}.capabilities.json", name));

    if !caps_path.exists() {
        anyhow::bail!(
            "Tool '{}' not found or has no capabilities file at {}",
            name,
            caps_path.display()
        );
    }

    let content = fs::read_to_string(&caps_path).await?;
    let caps = CapabilitiesFile::from_json(&content)
        .map_err(|e| anyhow::anyhow!("Invalid capabilities file: {}", e))?;

    let auth = caps.auth.ok_or_else(|| {
        anyhow::anyhow!(
            "Tool '{}' has no auth configuration.\n\
             The tool may not require authentication, or auth setup is not defined.",
            name
        )
    })?;

    let display_name = auth.display_name.as_deref().unwrap_or(&name);

    let header = format!("{} Authentication", display_name);
    println!();
    println!("╔════════════════════════════════════════════════════════════════╗");
    println!("║  {:^62}║", header);
    println!("╚════════════════════════════════════════════════════════════════╝");
    println!();

    let secrets_store = init_secrets_store().await?;

    let already_configured = secrets_store
        .exists(&user_id, &auth.secret_name)
        .await
        .unwrap_or(false);

    if already_configured {
        println!("  {} is already configured.", display_name);
        println!();
        print!("  Replace existing credentials? [y/N]: ");
        std::io::stdout().flush()?;

        let mut input = String::new();
        std::io::stdin().read_line(&mut input)?;

        if !input.trim().eq_ignore_ascii_case("y") {
            println!();
            println!("  Keeping existing credentials.");
            return Ok(());
        }
        println!();
    }

    if try_auth_from_env(secrets_store.as_ref(), &user_id, &auth, display_name).await? {
        return Ok(());
    }

    if let Some(ref oauth) = auth.oauth {
        let combined = combine_provider_scopes(&tools_dir, &auth.secret_name, oauth).await;
        if combined.scopes.len() > oauth.scopes.len() {
            let extra = combined.scopes.len() - oauth.scopes.len();
            println!(
                "  Including scopes from {} other installed tool(s) sharing this credential.",
                extra
            );
            println!();
        }
        return auth_tool_oauth(secrets_store.as_ref(), &user_id, &auth, &combined).await;
    }

    auth_tool_manual(secrets_store.as_ref(), &user_id, &auth).await
}

async fn try_auth_from_env(
    store: &(dyn SecretsStore + Send + Sync),
    user_id: &str,
    auth: &AuthCapabilitySchema,
    display_name: &str,
) -> anyhow::Result<bool> {
    let Some(env_var) = auth.env_var.as_ref() else {
        return Ok(false);
    };
    let Ok(token) = std::env::var(env_var) else {
        return Ok(false);
    };
    if token.is_empty() {
        return Ok(false);
    }

    println!("  Found {} in environment.", env_var);
    println!();

    if let Some(ref validation) = auth.validation_endpoint {
        print!("  Validating token...");
        std::io::stdout().flush()?;

        match validate_token(&token, validation, &auth.secret_name).await {
            Ok(()) => println!(" ✓"),
            Err(e) => {
                println!(" ✗");
                println!("  Validation failed: {}", e);
                println!();
                println!("  Falling back to manual entry...");
                return auth_tool_manual(store, user_id, auth).await.map(|_| true);
            }
        }
    }

    save_token(store, user_id, auth, &token, None, None).await?;
    print_success(display_name);
    Ok(true)
}

/// Manual token entry flow.
async fn auth_tool_manual(
    store: &(dyn SecretsStore + Send + Sync),
    user_id: &str,
    auth: &AuthCapabilitySchema,
) -> anyhow::Result<()> {
    let display_name = auth.display_name.as_deref().unwrap_or(&auth.secret_name);

    if let Some(ref instructions) = auth.instructions {
        println!("  Setup instructions:");
        println!();
        for line in instructions.lines() {
            println!("    {}", line);
        }
        println!();
    }

    if let Some(ref url) = auth.setup_url {
        print!("  Press Enter to open setup page (or 's' to skip): ");
        std::io::stdout().flush()?;

        let mut input = String::new();
        std::io::stdin().read_line(&mut input)?;

        if !input.trim().eq_ignore_ascii_case("s") {
            if let Err(e) = open::that(url) {
                println!("  Could not open browser: {}", e);
                println!("  Please open manually: {}", url);
            } else {
                println!("  Opening browser...");
            }
        }
        println!();
    }

    if let Some(ref hint) = auth.token_hint {
        println!("  Token format: {}", hint);
        println!();
    }

    print!("  Paste your token: ");
    std::io::stdout().flush()?;

    let token = read_hidden_input()?;
    println!();

    if token.is_empty() {
        println!("  No token provided. Aborting.");
        return Ok(());
    }

    if let Some(ref validation) = auth.validation_endpoint {
        print!("  Validating token...");
        std::io::stdout().flush()?;

        match validate_token(&token, validation, &auth.secret_name).await {
            Ok(()) => println!(" ✓"),
            Err(e) => {
                println!(" ✗");
                println!("  Validation failed: {}", e);
                println!();
                print!("  Save anyway? [y/N]: ");
                std::io::stdout().flush()?;

                let mut confirm = String::new();
                std::io::stdin().read_line(&mut confirm)?;

                if !confirm.trim().eq_ignore_ascii_case("y") {
                    println!("  Aborting.");
                    return Ok(());
                }
            }
        }
    }

    save_token(store, user_id, auth, &token, None, None).await?;
    print_success(display_name);
    Ok(())
}

/// Read input with hidden characters.
pub(super) fn read_hidden_input() -> anyhow::Result<String> {
    use crossterm::{
        event::{self, Event, KeyCode, KeyModifiers},
        terminal,
    };

    struct RawModeGuard;

    impl Drop for RawModeGuard {
        fn drop(&mut self) {
            let _ = terminal::disable_raw_mode();
        }
    }

    let mut input = String::new();
    terminal::enable_raw_mode()?;
    let _raw_mode_guard = RawModeGuard;

    loop {
        if let Event::Key(key_event) = event::read()? {
            match key_event.code {
                KeyCode::Enter => break,
                KeyCode::Backspace if !input.is_empty() => {
                    input.pop();
                    print!("\x08 \x08");
                    std::io::stdout().flush()?;
                }
                KeyCode::Backspace => {}
                KeyCode::Char('c') if key_event.modifiers.contains(KeyModifiers::CONTROL) => {
                    return Err(anyhow::anyhow!("Interrupted"));
                }
                KeyCode::Char(c) => {
                    input.push(c);
                    print!("*");
                    std::io::stdout().flush()?;
                }
                _ => {}
            }
        }
    }

    Ok(input)
}

/// Validate a token against the validation endpoint.
async fn validate_token(
    token: &str,
    validation: &ValidationEndpointSchema,
    _secret_name: &str,
) -> anyhow::Result<()> {
    crate::cli::oauth_defaults::validate_oauth_token(token, validation)
        .await
        .map_err(|e| anyhow::anyhow!("{}", e))
}

/// Save token to secrets store.
async fn save_token(
    store: &(dyn SecretsStore + Send + Sync),
    user_id: &str,
    auth: &AuthCapabilitySchema,
    token: &str,
    refresh_token: Option<&str>,
    expires_in: Option<u64>,
) -> anyhow::Result<()> {
    crate::cli::oauth_defaults::store_oauth_tokens(
        store,
        user_id,
        &auth.secret_name,
        auth.provider.as_deref(),
        token,
        refresh_token,
        expires_in,
        &[],
    )
    .await
    .map_err(|e| anyhow::anyhow!("{}", e))
}

/// Print success message.
fn print_success(display_name: &str) {
    println!();
    println!("  ✓ {} connected!", display_name);
    println!();
    println!("  The tool can now access the API.");
    println!();
}
