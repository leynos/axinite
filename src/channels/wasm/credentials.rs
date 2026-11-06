//! Inject credentials declared by a WASM channel.

use std::collections::HashSet;
use std::sync::Arc;

use crate::channels::wasm::WasmChannel;
use crate::channels::wasm::telemetry;
use crate::config::EnvContext;
use crate::secrets::{DecryptedSecret, SecretsStore};

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
        count +=
            inject_store_credentials(channel, secrets, channel_name, &mut injected_placeholders)
                .await?;
    }

    // 2. Fall back to the supplied environment for credentials missing from the
    // secrets store.
    count += inject_environment_credentials(
        channel,
        channel_name,
        &mut injected_placeholders,
        env_context,
    )
    .await;

    // The return value counts every injected secret, but the metric reports
    // whether the credentials the channel actually declares were resolved.
    // Unrelated store secrets must not mask a missing declared credential.
    let resolved_declared = resolved_declared_count(channel, &injected_placeholders);
    record_outcome(channel_name, declared, resolved_declared);
    Ok(count)
}

/// Inject every stored secret whose name carries the channel prefix.
async fn inject_store_credentials(
    channel: &Arc<WasmChannel>,
    secrets: &dyn SecretsStore,
    channel_name: &str,
    injected_placeholders: &mut HashSet<String>,
) -> anyhow::Result<usize> {
    let all_secrets = match secrets.list("default").await {
        Ok(all_secrets) => all_secrets,
        Err(e) => {
            telemetry::channel_credentials("list_error");
            return Err(anyhow::anyhow!("Failed to list secrets: {}", e));
        }
    };

    let prefix = format!("{}_", channel_name.to_ascii_lowercase());
    let mut count = 0;
    for secret_meta in all_secrets {
        if !secret_meta.name.to_ascii_lowercase().starts_with(&prefix) {
            continue;
        }
        let Some(decrypted) = decrypt_secret(secrets, &secret_meta.name).await else {
            continue;
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
    Ok(count)
}

/// Decrypt one stored secret, recording and skipping a decryption failure.
async fn decrypt_secret(secrets: &dyn SecretsStore, name: &str) -> Option<DecryptedSecret> {
    match secrets.get_decrypted("default", name).await {
        Ok(decrypted) => Some(decrypted),
        Err(e) => {
            telemetry::channel_credentials("decryption_error");
            tracing::warn!(
                secret = %name,
                error = %e,
                "Failed to decrypt secret for channel credential injection"
            );
            None
        }
    }
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

/// Count the declared credential mappings whose placeholders were resolved.
///
/// Only placeholders named by the channel's HTTP credential mappings count.
/// A store secret that does not correspond to a declared mapping is still
/// injected, but it does not satisfy the channel's declared contract.
fn resolved_declared_count(channel: &Arc<WasmChannel>, resolved: &HashSet<String>) -> usize {
    channel
        .capabilities()
        .tool_capabilities
        .http
        .as_ref()
        .map_or(0, |http| {
            http.credentials
                .values()
                .filter(|mapping| resolved.contains(&mapping.secret_name.to_uppercase()))
                .count()
        })
}

/// Select the fixed metric outcome for a completed injection attempt.
///
/// A channel that resolves none of its declared credentials is reported as
/// `missing`; a channel that declares none has nothing to report.
fn credential_outcome(declared: usize, resolved: usize) -> Option<&'static str> {
    if resolved > 0 {
        Some("injected")
    } else if declared > 0 {
        Some("missing")
    } else {
        None
    }
}

/// Record the outcome of one injection attempt and warn when a channel
/// declares credentials that none of its sources supplied.
fn record_outcome(channel_name: &str, declared: usize, resolved: usize) {
    let Some(outcome) = credential_outcome(declared, resolved) else {
        return;
    };
    telemetry::channel_credentials(outcome);
    if resolved == 0 {
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
    injected_placeholders: &mut HashSet<String>,
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
                injected_placeholders.insert(placeholder);
                count += 1;
            }
        }
    }

    count
}

#[cfg(test)]
mod tests;
