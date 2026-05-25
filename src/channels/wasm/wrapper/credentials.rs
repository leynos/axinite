use super::store::ResolvedHostCredential;
use crate::channels::wasm::capabilities::ChannelCapabilities;
use crate::secrets::SecretsStore;
use crate::tools::wasm::credential_injector::{InjectedCredentials, inject_credential};

/// Extract the hostname from a URL string.
///
/// Returns `None` for malformed URLs or non-HTTP(S) schemes.
pub(super) fn extract_host_from_url(url: &str) -> Option<String> {
    let parsed = url::Url::parse(url).ok()?;
    if !matches!(parsed.scheme(), "http" | "https") {
        return None;
    }
    parsed.host_str().map(|h| {
        h.strip_prefix('[')
            .and_then(|v| v.strip_suffix(']'))
            .unwrap_or(h)
            .to_lowercase()
    })
}

/// Pre-resolve host credentials for all HTTP capability mappings.
///
/// Called once per callback (in async context, before spawn_blocking) so the
/// synchronous WASM host function can inject credentials without needing async
/// access to the secrets store.
///
/// Silently skips credentials that can't be resolved (e.g., missing secrets).
/// The channel will get a 401/403 from the API, which is the expected UX when
/// auth hasn't been configured yet.
pub(super) async fn resolve_channel_host_credentials(
    capabilities: &ChannelCapabilities,
    store: Option<&(dyn SecretsStore + Send + Sync)>,
) -> Vec<ResolvedHostCredential> {
    let store = match store {
        Some(s) => s,
        None => return Vec::new(),
    };

    let http_cap = match &capabilities.tool_capabilities.http {
        Some(cap) => cap,
        None => return Vec::new(),
    };

    if http_cap.credentials.is_empty() {
        return Vec::new();
    }

    let mut resolved = Vec::new();

    for mapping in http_cap.credentials.values() {
        // Skip UrlPath credentials; they're handled by placeholder substitution
        if matches!(
            mapping.location,
            crate::secrets::CredentialLocation::UrlPath { .. }
        ) {
            continue;
        }

        let secret = match store.get_decrypted("default", &mapping.secret_name).await {
            Ok(s) => s,
            Err(e) => {
                tracing::debug!(
                    secret_name = %mapping.secret_name,
                    error = %e,
                    "Could not resolve credential for WASM channel (auth may not be configured)"
                );
                continue;
            }
        };

        let mut injected = InjectedCredentials::empty();
        inject_credential(&mut injected, &mapping.location, &secret);

        if injected.is_empty() {
            continue;
        }

        resolved.push(ResolvedHostCredential {
            host_patterns: mapping.host_patterns.clone(),
            headers: injected.headers,
            query_params: injected.query_params,
            secret_value: secret.expose().to_string(),
        });
    }

    if !resolved.is_empty() {
        tracing::debug!(
            count = resolved.len(),
            "Pre-resolved host credentials for WASM channel execution"
        );
    }

    resolved
}
