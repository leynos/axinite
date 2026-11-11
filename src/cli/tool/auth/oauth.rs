//! OAuth-specific helpers for tool authentication.
//!
//! These helpers keep browser-driven OAuth setup separate from the manual and
//! environment-backed authentication flows in the parent module.

use std::collections::HashSet;
use std::path::Path;

use crate::cli::oauth_defaults;
use crate::config::EnvContext;
use crate::secrets::SecretsStore;
use crate::tools::wasm::{AuthCapabilitySchema, CapabilitiesFile, OAuthConfigSchema};

use super::AuthToolContext;

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

            if let Some(scopes) = matching_oauth_scopes(&path, secret_name).await {
                all_scopes.extend(scopes);
            }
        }
    }

    let mut combined = base_oauth.clone();
    combined.scopes = all_scopes.into_iter().collect();
    combined.scopes.sort();
    combined
}

/// Read a capabilities file and return its OAuth scopes when its auth entry
/// targets `secret_name`; `None` for unreadable, unparsable, or unrelated
/// files.
async fn matching_oauth_scopes(path: &Path, secret_name: &str) -> Option<Vec<String>> {
    let content = tokio::fs::read_to_string(path).await.ok()?;
    let caps = CapabilitiesFile::from_json(&content).ok()?;
    let auth = caps.auth?;
    if auth.secret_name != secret_name {
        return None;
    }
    Some(auth.oauth?.scopes)
}
/// Run the interactive OAuth flow for a tool: bind the callback listener,
/// open the authorization URL, wait for the callback, then exchange the code
/// and store the returned tokens.
pub(super) async fn auth_tool_oauth(
    store: &(dyn SecretsStore + Send + Sync),
    context: &AuthToolContext<'_>,
    oauth: &OAuthConfigSchema,
) -> anyhow::Result<()> {
    let display_name = context.display_name;
    let (client_id, client_secret) =
        resolve_oauth_client_credentials(context.auth, oauth, context.env_context)?;

    println!("  Starting OAuth authentication...");
    println!();

    let listener = oauth_defaults::bind_callback_listener_from(context.env_context).await?;
    let redirect_uri = format!(
        "{}/callback",
        oauth_defaults::callback_url_from(context.env_context)
    );

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
        context.user_id,
        &context.auth.secret_name,
        context.auth.provider.as_deref(),
        &token_response.access_token,
        token_response.refresh_token.as_deref(),
        token_response.expires_in,
        &oauth.scopes,
    )
    .await?;

    super::print_success(display_name);
    Ok(())
}

fn resolve_oauth_client_credentials(
    auth: &AuthCapabilitySchema,
    oauth: &OAuthConfigSchema,
    env_context: &EnvContext,
) -> anyhow::Result<(String, Option<String>)> {
    let builtin = oauth_defaults::builtin_credentials(&auth.secret_name);
    let client_id = oauth
        .client_id
        .clone()
        .or_else(|| {
            oauth
                .client_id_env
                .as_deref()
                .and_then(|name| env_context.get_owned(name))
        })
        .or_else(|| {
            builtin
                .as_ref()
                .map(|credentials| credentials.client_id.to_string())
        })
        .ok_or_else(|| {
            anyhow::anyhow!(
                "OAuth client_id not configured.\n\
                 Set {} env var, or build with AXINITE_GOOGLE_CLIENT_ID.",
                oauth.client_id_env.as_deref().unwrap_or("the client_id")
            )
        })?;

    let client_secret = oauth
        .client_secret
        .clone()
        .or_else(|| {
            oauth
                .client_secret_env
                .as_deref()
                .and_then(|name| env_context.get_owned(name))
        })
        .or_else(|| {
            builtin
                .as_ref()
                .map(|credentials| credentials.client_secret.to_string())
        });

    Ok((client_id, client_secret))
}

#[cfg(test)]
mod tests {
    //! Unit tests for OAuth client credential resolution precedence.

    use super::resolve_oauth_client_credentials;
    use crate::config::EnvContext;
    use crate::tools::wasm::{AuthCapabilitySchema, OAuthConfigSchema};

    const CLIENT_ID_ENV: &str = "AXINITE_TEST_OAUTH_CLIENT_ID";
    const CLIENT_SECRET_ENV: &str = "AXINITE_TEST_OAUTH_CLIENT_SECRET";

    fn oauth_config(client_id: Option<&str>, client_secret: Option<&str>) -> OAuthConfigSchema {
        OAuthConfigSchema {
            authorization_url: "https://example.test/authorize".to_string(),
            token_url: "https://example.test/token".to_string(),
            client_id: client_id.map(str::to_string),
            client_id_env: Some(CLIENT_ID_ENV.to_string()),
            client_secret: client_secret.map(str::to_string),
            client_secret_env: Some(CLIENT_SECRET_ENV.to_string()),
            ..OAuthConfigSchema::default()
        }
    }

    fn auth(secret_name: &str) -> AuthCapabilitySchema {
        AuthCapabilitySchema {
            secret_name: secret_name.to_string(),
            ..AuthCapabilitySchema::default()
        }
    }

    /// Inline capability values outrank the snapshot and the bundled app.
    #[test]
    fn inline_credentials_take_precedence_over_snapshot_and_builtin() {
        let ctx = EnvContext::default()
            .with_env(CLIENT_ID_ENV, "snapshot-id")
            .with_env(CLIENT_SECRET_ENV, "snapshot-secret");

        let (client_id, client_secret) = resolve_oauth_client_credentials(
            &auth("google_oauth_token"),
            &oauth_config(Some("inline-id"), Some("inline-secret")),
            &ctx,
        )
        .expect("inline credentials must resolve");

        assert_eq!(client_id, "inline-id");
        assert_eq!(client_secret.as_deref(), Some("inline-secret"));
    }

    /// A provider without bundled credentials resolves from the snapshot.
    #[test]
    fn snapshot_environment_supplies_missing_credentials() {
        let ctx = EnvContext::default()
            .with_env(CLIENT_ID_ENV, "snapshot-id")
            .with_env(CLIENT_SECRET_ENV, "snapshot-secret");

        let (client_id, client_secret) = resolve_oauth_client_credentials(
            &auth("custom_oauth_token"),
            &oauth_config(None, None),
            &ctx,
        )
        .expect("snapshot credentials must resolve");

        assert_eq!(client_id, "snapshot-id");
        assert_eq!(client_secret.as_deref(), Some("snapshot-secret"));
    }

    /// The injected secret overlay is consulted when the environment is empty.
    #[test]
    fn injected_secret_overlay_supplies_credentials_when_environment_is_absent() {
        let ctx = EnvContext::default()
            .with_secret(CLIENT_ID_ENV, "overlay-id")
            .with_secret(CLIENT_SECRET_ENV, "overlay-secret");

        let (client_id, client_secret) = resolve_oauth_client_credentials(
            &auth("custom_oauth_token"),
            &oauth_config(None, None),
            &ctx,
        )
        .expect("overlay credentials must resolve");

        assert_eq!(client_id, "overlay-id");
        assert_eq!(client_secret.as_deref(), Some("overlay-secret"));
    }

    /// The bundled Google app is the last resort for its secret name.
    #[test]
    fn builtin_credentials_are_used_when_nothing_else_is_configured() {
        let ctx = EnvContext::default();

        let (client_id, client_secret) = resolve_oauth_client_credentials(
            &auth("google_oauth_token"),
            &oauth_config(None, None),
            &ctx,
        )
        .expect("bundled credentials must resolve");

        assert!(!client_id.is_empty());
        assert!(client_secret.is_some_and(|secret| !secret.is_empty()));
    }

    /// A missing client ID names the environment variable to set.
    #[test]
    fn missing_credentials_report_the_configured_environment_variable() {
        let ctx = EnvContext::default();

        let error = resolve_oauth_client_credentials(
            &auth("custom_oauth_token"),
            &oauth_config(None, None),
            &ctx,
        )
        .expect_err("a missing client_id must fail");

        let message = error.to_string();
        assert!(
            message.contains(CLIENT_ID_ENV),
            "the error must name the environment variable: {message}"
        );
    }

    /// Resolution must not fall back to the ambient process environment.
    ///
    /// The snapshot is empty, so a credential resolved here could only have
    /// come from the real process environment. To make the two sources
    /// distinguishable, the assertion runs in a child process whose ambient
    /// environment defines the credential variables: an ambient fallback
    /// resolves them and fails, while the snapshot-driven path ignores them.
    /// The variables are set on the child process only, so the parent never
    /// mutates its own environment.
    #[test]
    fn snapshot_credentials_ignore_the_ambient_environment() -> anyhow::Result<()> {
        const CHILD_MARKER: &str = "AXINITE_OAUTH_CREDENTIALS_AMBIENT_CHILD";

        if EnvContext::capture_ambient()
            .get_owned(CHILD_MARKER)
            .is_none()
        {
            let output = std::process::Command::new(std::env::current_exe()?)
                .args([
                    "--exact",
                    "cli::tool::auth::oauth::tests::snapshot_credentials_ignore_the_ambient_environment",
                    "--nocapture",
                    "--test-threads=1",
                ])
                .env(CHILD_MARKER, "1")
                .env(CLIENT_ID_ENV, "ambient-id")
                .env(CLIENT_SECRET_ENV, "ambient-secret")
                .output()?;
            let stdout = String::from_utf8_lossy(&output.stdout);
            assert!(
                output.status.success(),
                "child failed: {}\n{stdout}",
                output.status
            );
            assert!(
                stdout.contains("1 passed"),
                "child did not run the test: {stdout}"
            );
            return Ok(());
        }

        let ctx = EnvContext::default();
        let error = resolve_oauth_client_credentials(
            &auth("custom_oauth_token"),
            &oauth_config(None, None),
            &ctx,
        )
        .expect_err("an empty snapshot must not yield ambient credentials");

        assert!(
            error.to_string().contains(CLIENT_ID_ENV),
            "the ambient value must not be used: {error}"
        );
        Ok(())
    }
}
