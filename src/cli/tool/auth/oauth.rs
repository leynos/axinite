//! OAuth-specific helpers for tool authentication.
//!
//! These helpers keep browser-driven OAuth setup separate from the manual and
//! environment-backed authentication flows in the parent module.

use std::collections::HashSet;
use std::path::Path;

use crate::secrets::SecretsStore;
use crate::tools::wasm::{AuthCapabilitySchema, CapabilitiesFile, OAuthConfigSchema};

pub(super) async fn combine_provider_scopes(
    tools_dir: &Path,
    secret_name: &str,
    base_oauth: &OAuthConfigSchema,
) -> OAuthConfigSchema {
    let mut all_scopes: HashSet<String> = base_oauth.scopes.iter().cloned().collect();

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

pub(super) async fn auth_tool_oauth(
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

    super::print_success(display_name);
    Ok(())
}
