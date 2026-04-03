//! Credential helpers for live WASM channel activation.

use std::collections::HashSet;
use std::sync::Arc;

use crate::secrets::SecretsStore;

/// Auth readiness states for WASM channels.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ToolAuthState {
    /// No authentication required.
    NoAuth,
    /// All required secrets are present.
    Ready,
    /// Missing required secrets.
    NeedsSetup,
}

/// Inject channel credentials from secrets store and environment variables.
///
/// Looks for secrets matching the pattern `{channel_name}_*` and injects them
/// as credential placeholders (e.g., `telegram_bot_token` ->
/// `{TELEGRAM_BOT_TOKEN}`).
///
/// Falls back to environment variables starting with the uppercase channel name
/// prefix (e.g., `TELEGRAM_` for channel `telegram`) for missing credentials.
///
/// Returns the number of credentials injected.
pub(super) async fn inject_channel_credentials_from_secrets(
    channel: &Arc<crate::channels::wasm::WasmChannel>,
    secrets: Option<&dyn SecretsStore>,
    channel_name: &str,
    user_id: &str,
) -> Result<usize, String> {
    let mut count = 0;
    let mut injected_placeholders = HashSet::new();

    // 1. Try injecting from persistent secrets store if available
    if let Some(secrets) = secrets {
        let all_secrets = secrets
            .list(user_id)
            .await
            .map_err(|e| format!("Failed to list secrets: {}", e))?;

        let prefix = format!("{}_", channel_name.to_ascii_lowercase());

        for secret_meta in all_secrets {
            if !secret_meta.name.to_ascii_lowercase().starts_with(&prefix) {
                continue;
            }

            let decrypted = match secrets.get_decrypted(user_id, &secret_meta.name).await {
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
            channel
                .set_credential(&placeholder, decrypted.expose().to_string())
                .await;
            injected_placeholders.insert(placeholder);
            count += 1;
        }
    }

    // 2. Fallback to environment variables for missing credentials
    count += inject_env_credentials(channel, channel_name, &injected_placeholders).await;

    Ok(count)
}

/// Inject missing credentials from environment variables.
///
/// Only environment variables starting with the uppercase channel name prefix
/// (e.g., `TELEGRAM_` for channel `telegram`) are considered for security.
async fn inject_env_credentials(
    channel: &Arc<crate::channels::wasm::WasmChannel>,
    channel_name: &str,
    already_injected: &HashSet<String>,
) -> usize {
    if channel_name.trim().is_empty() {
        return 0;
    }

    let caps = channel.capabilities();
    let Some(ref http_cap) = caps.tool_capabilities.http else {
        return 0;
    };

    let placeholders: Vec<String> = http_cap
        .credentials
        .values()
        .map(|m| m.secret_name.to_uppercase())
        .collect();

    let resolved = resolve_env_credentials(&placeholders, channel_name, already_injected);
    let count = resolved.len();
    for (placeholder, value) in resolved {
        channel.set_credential(&placeholder, value).await;
    }
    count
}

/// Pure helper: from a list of credential placeholder names, return those that
/// pass the channel-prefix security check and have a non-empty env var value.
///
/// Placeholders already covered by the secrets store (`already_injected`) are
/// skipped. Only names starting with `{CHANNEL_NAME}_` are allowed to prevent
/// a WASM channel from reading unrelated host credentials (e.g.
/// `AWS_SECRET_ACCESS_KEY`).
pub(super) fn resolve_env_credentials(
    placeholders: &[String],
    channel_name: &str,
    already_injected: &HashSet<String>,
) -> Vec<(String, String)> {
    if channel_name.trim().is_empty() {
        return Vec::new();
    }

    let prefix = format!("{}_", channel_name.to_ascii_uppercase());
    let mut out = Vec::new();

    for placeholder in placeholders {
        if already_injected.contains(placeholder) {
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
        if let Ok(value) = std::env::var(placeholder)
            && !value.is_empty()
        {
            out.push((placeholder.clone(), value));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_security_prefix_check() {
        // Placeholders that don't start with the channel prefix must be rejected.
        // All env var names are prefixed with ICTEST1_ to avoid CI collisions.
        let placeholders = vec![
            "ICTEST1_BOT_TOKEN".to_string(), // valid: matches channel prefix
            "ICTEST2_TOKEN".to_string(),     // invalid: wrong channel prefix
            "ICTEST1_UNRELATED_OTHER".to_string(), // valid prefix, but env var not set — not injected
        ];
        let already_injected = std::collections::HashSet::new();

        unsafe { std::env::set_var("ICTEST1_BOT_TOKEN", "good-secret") };
        unsafe { std::env::set_var("ICTEST2_TOKEN", "bad-secret") };
        // ICTEST1_UNRELATED_OTHER intentionally not set — tests both prefix rejection and absence

        let resolved = resolve_env_credentials(&placeholders, "ictest1", &already_injected);

        // Only ICTEST1_BOT_TOKEN passes the prefix check for channel "ictest1"
        assert_eq!(resolved.len(), 1);
        assert_eq!(resolved[0].0, "ICTEST1_BOT_TOKEN");
        assert_eq!(resolved[0].1, "good-secret");

        unsafe { std::env::remove_var("ICTEST1_BOT_TOKEN") };
        unsafe { std::env::remove_var("ICTEST2_TOKEN") };
    }

    #[test]
    fn test_already_injected_skipped() {
        // Use unique env var names (ictest3_*) to avoid interference with other tests.
        let placeholders = vec!["ICTEST3_TOKEN".to_string()];
        let mut already_injected = std::collections::HashSet::new();
        already_injected.insert("ICTEST3_TOKEN".to_string());

        unsafe { std::env::set_var("ICTEST3_TOKEN", "secret") };

        let resolved = resolve_env_credentials(&placeholders, "ictest3", &already_injected);

        // Already covered by secrets store — env var must be skipped
        assert!(resolved.is_empty());

        unsafe { std::env::remove_var("ICTEST3_TOKEN") };
    }

    #[test]
    fn test_missing_env_var_not_injected() {
        // Use unique env var names (ictest4_*) to avoid interference with other tests.
        let placeholders = vec!["ICTEST4_TOKEN".to_string()];
        let already_injected = std::collections::HashSet::new();

        unsafe { std::env::remove_var("ICTEST4_TOKEN") };

        let resolved = resolve_env_credentials(&placeholders, "ictest4", &already_injected);

        assert!(resolved.is_empty());
    }

    #[test]
    fn test_empty_env_var_not_injected() {
        // An env var that exists but is empty must not be injected.
        // Use unique env var names (ictest5_*) to avoid interference with other tests.
        let placeholders = vec!["ICTEST5_TOKEN".to_string()];
        let already_injected = std::collections::HashSet::new();

        unsafe { std::env::set_var("ICTEST5_TOKEN", "") };

        let resolved = resolve_env_credentials(&placeholders, "ictest5", &already_injected);

        assert!(resolved.is_empty());

        unsafe { std::env::remove_var("ICTEST5_TOKEN") };
    }

    #[test]
    fn test_empty_channel_name_returns_nothing() {
        // An empty channel name must never match any env var (prefix would be "_").
        let placeholders = vec!["_TOKEN".to_string(), "ICTEST6_TOKEN".to_string()];
        let already_injected = std::collections::HashSet::new();

        unsafe { std::env::set_var("_TOKEN", "bad") };
        unsafe { std::env::set_var("ICTEST6_TOKEN", "bad") };

        let resolved = resolve_env_credentials(&placeholders, "", &already_injected);

        assert!(resolved.is_empty(), "empty channel name must match nothing");

        unsafe { std::env::remove_var("_TOKEN") };
        unsafe { std::env::remove_var("ICTEST6_TOKEN") };
    }
}
