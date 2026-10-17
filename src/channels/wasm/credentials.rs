//! Inject credentials declared by a WASM channel.

use std::collections::HashSet;
use std::sync::Arc;

use crate::channels::wasm::WasmChannel;
use crate::config::EnvContext;
use crate::secrets::SecretsStore;

/// Inject credentials for a channel based on naming convention.
///
/// Looks for secrets matching the pattern `{channel_name}_*` and injects them
/// as credential placeholders (e.g., `telegram_bot_token` -> `{TELEGRAM_BOT_TOKEN}`).
///
/// Falls back to environment variables starting with the uppercase channel name
/// prefix (e.g., `TELEGRAM_`) for credentials missing from the secrets store.
///
/// Returns the number of credentials injected.
pub async fn inject_channel_credentials(
    channel: &Arc<WasmChannel>,
    secrets: Option<&dyn SecretsStore>,
    channel_name: &str,
) -> anyhow::Result<usize> {
    inject_channel_credentials_from_context(
        channel,
        secrets,
        channel_name,
        &EnvContext::capture_ambient(),
    )
    .await
}

/// Inject channel credentials using an explicit environment snapshot.
pub async fn inject_channel_credentials_from_context(
    channel: &Arc<WasmChannel>,
    secrets: Option<&dyn SecretsStore>,
    channel_name: &str,
    env_context: &EnvContext,
) -> anyhow::Result<usize> {
    if channel_name.trim().is_empty() {
        return Ok(0);
    }

    let mut count = 0;
    let mut injected_placeholders = HashSet::new();

    // 1. Try injecting from persistent secrets store if available
    if let Some(secrets) = secrets {
        let all_secrets = secrets
            .list("default")
            .await
            .map_err(|e| anyhow::anyhow!("Failed to list secrets: {}", e))?;

        let prefix = format!("{}_", channel_name.to_ascii_lowercase());

        for secret_meta in all_secrets {
            if !secret_meta.name.to_ascii_lowercase().starts_with(&prefix) {
                continue;
            }

            let decrypted = match secrets.get_decrypted("default", &secret_meta.name).await {
                Ok(d) => d,
                Err(e) => {
                    tracing::warn!(
                        secret = %secret_meta.name,
                        error = %e,
                        "Failed to decrypt secret for channel credential injection"
                    );
                    continue;
                }
            };

            let placeholder = secret_meta.name.to_uppercase();

            tracing::debug!(
                channel = %channel_name,
                secret = %secret_meta.name,
                placeholder = %placeholder,
                "Injecting credential"
            );

            channel
                .set_credential(&placeholder, decrypted.expose().to_string())
                .await;
            injected_placeholders.insert(placeholder);
            count += 1;
        }
    }

    // 2. Fall back to the supplied environment for credentials missing from the
    // secrets store.
    count +=
        inject_environment_credentials(channel, channel_name, &injected_placeholders, env_context)
            .await;

    Ok(count)
}

/// Inject matching environment-backed credentials, skipping secrets already
/// supplied by the persistent store.
async fn inject_environment_credentials(
    channel: &Arc<WasmChannel>,
    channel_name: &str,
    injected_placeholders: &HashSet<String>,
    env_context: &EnvContext,
) -> usize {
    let prefix = format!("{}_", channel_name.to_ascii_uppercase());
    let caps = channel.capabilities();
    let mut count = 0;
    if let Some(ref http_cap) = caps.tool_capabilities.http {
        for cred_mapping in http_cap.credentials.values() {
            let placeholder = cred_mapping.secret_name.to_uppercase();
            if injected_placeholders.contains(&placeholder) {
                continue;
            }
            if !placeholder.starts_with(&prefix) {
                tracing::warn!(
                    channel = %channel_name,
                    placeholder = %placeholder,
                    "Ignoring non-prefixed credential placeholder in environment fallback"
                );
                continue;
            }
            if let Some(env_value) = env_context.get_owned(&placeholder) {
                tracing::debug!(
                    channel = %channel_name,
                    placeholder = %placeholder,
                    "Injecting credential from environment variable"
                );
                channel.set_credential(&placeholder, env_value).await;
                count += 1;
            }
        }
    }

    count
}
