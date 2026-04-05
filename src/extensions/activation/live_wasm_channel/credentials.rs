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
    user_id: &str,
) -> Result<usize, String> {
    let mut count = 0;
    let mut injected_placeholders = HashSet::new();
    let allowed_keys = allowed_credential_keys(channel);

    // 1. Try injecting from persistent secrets store if available
    if let Some(secrets) = secrets {
        let all_secrets = secrets
            .list(user_id)
            .await
            .map_err(|e| format!("Failed to list secrets: {}", e))?;

        for secret_meta in all_secrets {
            let placeholder = secret_meta.name.to_uppercase();
            if !allowed_keys.contains(&placeholder) {
                continue;
            }

            let decrypted = match secrets.get_decrypted(user_id, &secret_meta.name).await {
                Ok(d) => d,
                Err(crate::secrets::SecretError::NotFound(_)) => {
                    tracing::warn!(
                        secret = %secret_meta.name,
                        "Secret disappeared during channel credential injection"
                    );
                    continue;
                }
                Err(e) => {
                    return Err(format!(
                        "Failed to decrypt secret '{}' for channel credential injection: {}",
                        secret_meta.name, e
                    ));
                }
            };

            channel
                .set_credential(&placeholder, decrypted.expose().to_string())
                .await;
            injected_placeholders.insert(placeholder);
            count += 1;
        }
    }

    // 2. Fallback to environment variables for missing credentials
    count += inject_env_credentials(channel, &allowed_keys, &injected_placeholders).await;

    Ok(count)
}

/// Inject missing credentials from environment variables.
///
/// Only environment variables starting with the uppercase channel name prefix
/// (e.g., `TELEGRAM_` for channel `telegram`) are considered for security.
async fn inject_env_credentials(
    channel: &Arc<crate::channels::wasm::WasmChannel>,
    allowed_keys: &HashSet<String>,
    already_injected: &HashSet<String>,
) -> usize {
    if allowed_keys.is_empty() {
        return 0;
    }

    let resolved = resolve_env_credentials(allowed_keys, already_injected);
    let count = resolved.len();
    for (placeholder, value) in resolved {
        channel.set_credential(&placeholder, value).await;
    }
    count
}

/// Pure helper: from a list of credential placeholder names, return those that
/// are explicitly allowed by the channel capabilities and have a non-empty env var value.
///
/// Placeholders already covered by the secrets store (`already_injected`) are
/// skipped. Only credential names declared in the channel capabilities are allowed
/// to prevent a WASM channel from reading unrelated host credentials (e.g.
/// `AWS_SECRET_ACCESS_KEY`) via a crafted channel name.
pub(super) fn resolve_env_credentials(
    allowed_keys: &HashSet<String>,
    already_injected: &HashSet<String>,
) -> Vec<(String, String)> {
    resolve_env_credentials_with_reader(allowed_keys, already_injected, |placeholder| {
        std::env::var(placeholder).ok()
    })
}

fn resolve_env_credentials_with_reader<F>(
    allowed_keys: &HashSet<String>,
    already_injected: &HashSet<String>,
    env_reader: F,
) -> Vec<(String, String)>
where
    F: Fn(&str) -> Option<String>,
{
    if allowed_keys.is_empty() {
        return Vec::new();
    }

    let mut out = Vec::new();

    for placeholder in allowed_keys {
        if already_injected.contains(placeholder) {
            continue;
        }
        if let Some(value) = env_reader(placeholder)
            && !value.is_empty()
        {
            out.push((placeholder.clone(), value));
        }
    }
    out
}

fn allowed_credential_keys(channel: &Arc<crate::channels::wasm::WasmChannel>) -> HashSet<String> {
    let caps = channel.capabilities();
    caps.tool_capabilities
        .http
        .iter()
        .flat_map(|http_cap| http_cap.credentials.values())
        .map(|mapping| mapping.secret_name.to_uppercase())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    #[test]
    fn test_security_prefix_check() {
        let allowed_keys = HashSet::from(["ICTEST1_BOT_TOKEN".to_string()]);
        let already_injected = std::collections::HashSet::new();
        let env = HashMap::from([
            ("ICTEST1_BOT_TOKEN".to_string(), "good-secret".to_string()),
            ("ICTEST2_TOKEN".to_string(), "bad-secret".to_string()),
        ]);

        let resolved =
            resolve_env_credentials_with_reader(&allowed_keys, &already_injected, |key| {
                env.get(key).cloned()
            });

        assert_eq!(resolved.len(), 1);
        assert_eq!(resolved[0].0, "ICTEST1_BOT_TOKEN");
        assert_eq!(resolved[0].1, "good-secret");
    }

    #[test]
    fn test_already_injected_skipped() {
        let allowed_keys = HashSet::from(["ICTEST3_TOKEN".to_string()]);
        let mut already_injected = std::collections::HashSet::new();
        already_injected.insert("ICTEST3_TOKEN".to_string());
        let env = HashMap::from([("ICTEST3_TOKEN".to_string(), "secret".to_string())]);

        let resolved =
            resolve_env_credentials_with_reader(&allowed_keys, &already_injected, |key| {
                env.get(key).cloned()
            });

        assert!(resolved.is_empty());
    }

    #[test]
    fn test_missing_env_var_not_injected() {
        let allowed_keys = HashSet::from(["ICTEST4_TOKEN".to_string()]);
        let already_injected = std::collections::HashSet::new();
        let env: HashMap<String, String> = HashMap::new();

        let resolved =
            resolve_env_credentials_with_reader(&allowed_keys, &already_injected, |key| {
                env.get(key).cloned()
            });

        assert!(resolved.is_empty());
    }

    #[test]
    fn test_empty_env_var_not_injected() {
        let allowed_keys = HashSet::from(["ICTEST5_TOKEN".to_string()]);
        let already_injected = std::collections::HashSet::new();
        let env = HashMap::from([("ICTEST5_TOKEN".to_string(), String::new())]);

        let resolved =
            resolve_env_credentials_with_reader(&allowed_keys, &already_injected, |key| {
                env.get(key).cloned()
            });

        assert!(resolved.is_empty());
    }

    #[test]
    fn test_empty_allowed_keys_returns_nothing() {
        let allowed_keys = std::collections::HashSet::new();
        let already_injected = std::collections::HashSet::new();
        let env = HashMap::from([
            ("_TOKEN".to_string(), "bad".to_string()),
            ("ICTEST6_TOKEN".to_string(), "bad".to_string()),
        ]);

        let resolved =
            resolve_env_credentials_with_reader(&allowed_keys, &already_injected, |key| {
                env.get(key).cloned()
            });

        assert!(resolved.is_empty(), "empty allow-list must match nothing");
    }
}
