//! Inject credentials declared by a WASM channel.

use std::collections::HashSet;
use std::sync::Arc;

use crate::channels::wasm::WasmChannel;
use crate::channels::wasm::telemetry;
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

    let declared = declared_credential_count(channel);
    let mut count = 0;
    let mut injected_placeholders = HashSet::new();

    // 1. Try injecting from persistent secrets store if available
    if let Some(secrets) = secrets {
        let all_secrets = match secrets.list("default").await {
            Ok(all_secrets) => all_secrets,
            Err(e) => {
                telemetry::channel_credentials("list_error");
                return Err(anyhow::anyhow!("Failed to list secrets: {}", e));
            }
        };

        let prefix = format!("{}_", channel_name.to_ascii_lowercase());

        for secret_meta in all_secrets {
            if !secret_meta.name.to_ascii_lowercase().starts_with(&prefix) {
                continue;
            }

            let decrypted = match secrets.get_decrypted("default", &secret_meta.name).await {
                Ok(d) => d,
                Err(e) => {
                    telemetry::channel_credentials("decryption_error");
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

    record_outcome(channel_name, declared, count);
    Ok(count)
}

/// Count the credentials the channel's HTTP capability declares.
fn declared_credential_count(channel: &Arc<WasmChannel>) -> usize {
    channel
        .capabilities()
        .tool_capabilities
        .http
        .as_ref()
        .map_or(0, |http| http.credentials.len())
}

/// Select the fixed metric outcome for a completed injection attempt.
///
/// A channel that resolves none of its declared credentials is reported as
/// `missing`; a channel that declares none has nothing to report.
fn credential_outcome(declared: usize, injected: usize) -> Option<&'static str> {
    if injected > 0 {
        Some("injected")
    } else if declared > 0 {
        Some("missing")
    } else {
        None
    }
}

/// Record the outcome of one injection attempt and warn when a channel
/// declares credentials that none of its sources supplied.
fn record_outcome(channel_name: &str, declared: usize, injected: usize) {
    let Some(outcome) = credential_outcome(declared, injected) else {
        return;
    };
    telemetry::channel_credentials(outcome);
    if injected == 0 {
        tracing::warn!(
            channel = %channel_name,
            declared,
            "Channel declares credentials but none resolved"
        );
    }
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

#[cfg(test)]
mod tests;
