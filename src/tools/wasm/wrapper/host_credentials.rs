//! Pre-resolution of host-side credentials for WASM tools.

use super::credentials::refresh_oauth_token;
use super::store::ResolvedHostCredential;
use super::*;
use crate::secrets::CredentialMapping;

/// Pre-resolve credentials for all HTTP capability mappings.
///
/// Called once per tool execution (in async context, before spawn_blocking)
/// so that the synchronous WASM host function can inject credentials
/// without needing async access to the secrets store.
///
/// If an `OAuthRefreshConfig` is provided and the access token is expired
/// (or within 5 minutes of expiry), attempts a transparent refresh first.
///
/// Silently skips credentials that can't be resolved (e.g., missing secrets).
/// The tool will get a 401/403 from the API, which is the expected UX when
/// auth hasn't been configured yet.
pub(super) async fn resolve_host_credentials(
    capabilities: &Capabilities,
    store: Option<&(dyn SecretsStore + Send + Sync)>,
    user_id: &str,
    oauth_refresh: Option<&OAuthRefreshConfig>,
) -> Vec<ResolvedHostCredential> {
    let Some(store) = store else {
        return Vec::new();
    };

    // Check if the access token needs refreshing before resolving credentials.
    // This runs once per tool execution, keeping the hot path (credential injection
    // inside WASM) synchronous and allocation-free.
    if let Some(config) = oauth_refresh {
        refresh_if_needed(store, user_id, config).await;
    }

    let Some(http_cap) = &capabilities.http else {
        return Vec::new();
    };
    resolve_mappings(store, user_id, http_cap.credentials.values()).await
}

/// Refresh the OAuth access token when it is expired or near expiry.
async fn refresh_if_needed(
    store: &(dyn SecretsStore + Send + Sync),
    user_id: &str,
    config: &OAuthRefreshConfig,
) {
    if !access_token_needs_refresh(store, user_id, config).await {
        return;
    }
    tracing::debug!(
        secret_name = %config.secret_name,
        "Access token expired or near expiry, attempting refresh"
    );
    refresh_oauth_token(store, user_id, config).await;
}

/// Resolve every mapping, keeping the ones that yield credentials.
async fn resolve_mappings<'a>(
    store: &(dyn SecretsStore + Send + Sync),
    user_id: &str,
    mappings: impl Iterator<Item = &'a CredentialMapping>,
) -> Vec<ResolvedHostCredential> {
    let mut resolved = Vec::new();

    for mapping in mappings {
        if let Some(credential) = resolve_mapping_credential(store, user_id, mapping).await {
            resolved.push(credential);
        }
    }

    if !resolved.is_empty() {
        tracing::debug!(
            count = resolved.len(),
            "Pre-resolved host credentials for WASM tool execution"
        );
    }

    resolved
}

/// Decide whether the stored access token must be refreshed before use.
///
/// True when the token is within five minutes of expiry or the store reports it
/// expired. A token without `expires_at` is a legacy one and is never refreshed;
/// not-found and other store errors skip the refresh and leave the normal flow
/// to handle them.
async fn access_token_needs_refresh(
    store: &(dyn SecretsStore + Send + Sync),
    user_id: &str,
    config: &OAuthRefreshConfig,
) -> bool {
    match store.get(user_id, &config.secret_name).await {
        Ok(secret) => secret.expires_at.is_some_and(|expires_at| {
            let buffer = chrono::Duration::minutes(5);
            expires_at - buffer < chrono::Utc::now()
        }),
        Err(crate::secrets::SecretError::Expired) => true,
        Err(_) => false,
    }
}

/// Resolve one credential mapping to host-side injection data.
///
/// Skips `UrlPath` credentials (handled by placeholder substitution), secrets
/// that cannot be resolved, and mappings that inject nothing.
async fn resolve_mapping_credential(
    store: &(dyn SecretsStore + Send + Sync),
    user_id: &str,
    mapping: &CredentialMapping,
) -> Option<ResolvedHostCredential> {
    if matches!(
        mapping.location,
        crate::secrets::CredentialLocation::UrlPath { .. }
    ) {
        return None;
    }

    let secret = match store.get_decrypted(user_id, &mapping.secret_name).await {
        Ok(s) => s,
        Err(e) => {
            tracing::debug!(
                secret_name = %mapping.secret_name,
                error = %e,
                "Could not resolve credential for WASM tool (auth may not be configured)"
            );
            return None;
        }
    };

    let mut injected = InjectedCredentials::empty();
    inject_credential(&mut injected, &mapping.location, &secret);

    if injected.is_empty() {
        return None;
    }

    Some(ResolvedHostCredential {
        host_patterns: mapping.host_patterns.clone(),
        headers: injected.headers,
        query_params: injected.query_params,
        secret_value: secret.expose().to_string(),
    })
}
