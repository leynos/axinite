use std::io::Write;
use std::path::{Path, PathBuf};

use tokio::fs;

use crate::secrets::SecretsStore;
use crate::tools::wasm::{
    AuthCapabilitySchema, CapabilitiesFile, OAuthConfigSchema, ValidationEndpointSchema,
};

use super::default_tools_dir;
use super::init_secrets_store;
use super::printing::validate_tool_name;

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

    if let Some(ref env_var) = auth.env_var
        && let Ok(token) = std::env::var(env_var)
        && !token.is_empty()
    {
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
                    return auth_tool_manual(secrets_store.as_ref(), &user_id, &auth).await;
                }
            }
        }

        save_token(secrets_store.as_ref(), &user_id, &auth, &token, None, None).await?;
        print_success(display_name);
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

/// Scan the tools directory for all capabilities files sharing the same secret name
/// and combine their OAuth scopes.
async fn combine_provider_scopes(
    tools_dir: &Path,
    secret_name: &str,
    base_oauth: &OAuthConfigSchema,
) -> OAuthConfigSchema {
    let mut all_scopes: std::collections::HashSet<String> =
        base_oauth.scopes.iter().cloned().collect();

    if let Ok(mut entries) = tokio::fs::read_dir(tools_dir).await {
        while let Ok(Some(entry)) = entries.next_entry().await {
            let path = entry.path();
            if path.extension().and_then(|ext| ext.to_str()) != Some("json") {
                continue;
            }
            let name = path
                .file_name()
                .and_then(|file_name| file_name.to_str())
                .unwrap_or_default();
            if !name.ends_with(".capabilities.json") {
                continue;
            }

            if let Ok(content) = tokio::fs::read_to_string(&path).await
                && let Ok(caps) = CapabilitiesFile::from_json(&content)
                && let Some(auth) = &caps.auth
                && auth.secret_name == secret_name
                && let Some(oauth) = &auth.oauth
            {
                all_scopes.extend(oauth.scopes.iter().cloned());
            }
        }
    }

    let mut combined = base_oauth.clone();
    combined.scopes = all_scopes.into_iter().collect();
    combined.scopes.sort();
    combined
}

/// OAuth browser-based login flow.
async fn auth_tool_oauth(
    store: &(dyn SecretsStore + Send + Sync),
    user_id: &str,
    auth: &AuthCapabilitySchema,
    oauth: &OAuthConfigSchema,
) -> anyhow::Result<()> {
    use crate::cli::oauth_defaults;

    let display_name = auth.display_name.as_deref().unwrap_or(&auth.secret_name);
    let builtin = oauth_defaults::builtin_credentials(&auth.secret_name);

    let client_id = oauth
        .client_id
        .clone()
        .or_else(|| {
            oauth
                .client_id_env
                .as_ref()
                .and_then(|env| std::env::var(env).ok())
        })
        .or_else(|| {
            builtin
                .as_ref()
                .map(|credentials| credentials.client_id.to_string())
        })
        .ok_or_else(|| {
            anyhow::anyhow!(
                "OAuth client_id not configured.\n\
                 Set {} env var, or build with IRONCLAW_GOOGLE_CLIENT_ID.",
                oauth.client_id_env.as_deref().unwrap_or("the client_id")
            )
        })?;

    let client_secret = oauth
        .client_secret
        .clone()
        .or_else(|| {
            oauth
                .client_secret_env
                .as_ref()
                .and_then(|env| std::env::var(env).ok())
        })
        .or_else(|| {
            builtin
                .as_ref()
                .map(|credentials| credentials.client_secret.to_string())
        });

    println!("  Starting OAuth authentication...");
    println!();

    let listener = oauth_defaults::bind_callback_listener().await?;
    let redirect_uri = format!("{}/callback", oauth_defaults::callback_url());

    let oauth_result = oauth_defaults::build_oauth_url(
        &oauth.authorization_url,
        &client_id,
        &redirect_uri,
        &oauth.scopes,
        oauth.use_pkce,
        &oauth.extra_params,
    )?;
    let code_verifier = oauth_result.code_verifier;

    println!("  Opening browser for {} login...", display_name);
    println!();

    if let Err(e) = open::that(&oauth_result.url) {
        println!("  Could not open browser: {}", e);
        println!("  Please open this URL manually:");
        println!("  {}", oauth_result.url);
    }

    println!("  Waiting for authorization...");

    let code = oauth_defaults::wait_for_callback(
        listener,
        "/callback",
        "code",
        display_name,
        Some(&oauth_result.state),
    )
    .await?;

    println!();
    println!("  Exchanging code for token...");

    let token_response = oauth_defaults::exchange_oauth_code(
        &oauth.token_url,
        &client_id,
        client_secret.as_deref(),
        &code,
        &redirect_uri,
        code_verifier.as_deref(),
        &oauth.access_token_field,
    )
    .await?;

    oauth_defaults::store_oauth_tokens(
        store,
        user_id,
        &auth.secret_name,
        auth.provider.as_deref(),
        &token_response.access_token,
        token_response.refresh_token.as_deref(),
        token_response.expires_in,
        &oauth.scopes,
    )
    .await?;

    println!();
    println!("  ✓ {} connected!", display_name);
    println!();
    println!("  The tool can now access the API.");
    println!();

    Ok(())
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

    let mut input = String::new();
    terminal::enable_raw_mode()?;

    loop {
        if let Event::Key(key_event) = event::read()? {
            match key_event.code {
                KeyCode::Enter => break,
                KeyCode::Backspace => {
                    if !input.is_empty() {
                        input.pop();
                        print!("\x08 \x08");
                        std::io::stdout().flush()?;
                    }
                }
                KeyCode::Char('c') if key_event.modifiers.contains(KeyModifiers::CONTROL) => {
                    terminal::disable_raw_mode()?;
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

    terminal::disable_raw_mode()?;
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
