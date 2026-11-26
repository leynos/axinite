//! Host-credential resolution for WASM tools, including transparent
//! OAuth token refresh before execution.

use super::http::reject_private_ip;
use super::store::ResolvedHostCredential;
use super::*;
use crate::secrets::CredentialMapping;

/// Refresh an expired OAuth access token using the stored refresh token.
///
/// Posts to the provider's token endpoint with `grant_type=refresh_token`,
/// then stores the new access token (with expiry) and rotated refresh token
/// (if the provider returns one).
///
/// SSRF defence: `token_url` originates from a tool's capabilities JSON, so
/// a malicious tool could point it at an internal service to exfiltrate the
/// refresh token. We require HTTPS, reject private/loopback IPs (including
/// DNS-resolved), and disable redirects.
///
/// Returns `true` if the refresh succeeded, `false` otherwise.
async fn refresh_oauth_token(
    store: &(dyn SecretsStore + Send + Sync),
    user_id: &str,
    config: &OAuthRefreshConfig,
) -> bool {
    if !token_url_is_safe(&config.token_url) {
        return false;
    }

    let refresh_name = refresh_secret_name(config);
    let refresh_secret = match store.get_decrypted(user_id, &refresh_name).await {
        Ok(s) => s,
        Err(e) => {
            tracing::debug!(
                secret_name = %refresh_name,
                error = %e,
                "No refresh token available, skipping token refresh"
            );
            return false;
        }
    };

    let Some(token_data) = request_token_refresh(config, refresh_secret.expose()).await else {
        return false;
    };

    if !store_refreshed_tokens(store, user_id, config, &token_data).await {
        return false;
    }

    tracing::info!(
        secret_name = %config.secret_name,
        "OAuth access token refreshed successfully"
    );
    true
}

/// Name of the secret that holds the refresh token for `config`'s access token.
fn refresh_secret_name(config: &OAuthRefreshConfig) -> String {
    format!("{}_refresh_token", config.secret_name)
}

/// SSRF defence: `token_url` comes from the tool's capabilities file, so it
/// must be HTTPS and must not point at a private or internal address.
pub(super) fn token_url_is_safe(token_url: &str) -> bool {
    if !token_url.starts_with("https://") {
        tracing::warn!(
            token_url = %token_url,
            "OAuth token_url must use HTTPS, refusing token refresh"
        );
        return false;
    }
    if let Err(reason) = reject_private_ip(token_url) {
        tracing::warn!(
            token_url = %token_url,
            reason = %reason,
            "OAuth token_url points to a private/internal IP, refusing token refresh"
        );
        return false;
    }
    true
}

/// Post the refresh grant to the token endpoint and return the parsed JSON reply.
async fn request_token_refresh(
    config: &OAuthRefreshConfig,
    refresh_token: &str,
) -> Option<serde_json::Value> {
    let client = match reqwest::Client::builder()
        .timeout(Duration::from_secs(15))
        .redirect(reqwest::redirect::Policy::none())
        .build()
    {
        Ok(c) => c,
        Err(e) => {
            tracing::warn!(error = %e, "Failed to build HTTP client for token refresh");
            return None;
        }
    };

    let mut params = vec![
        ("grant_type", "refresh_token".to_string()),
        ("refresh_token", refresh_token.to_string()),
        ("client_id", config.client_id.clone()),
    ];
    if let Some(ref secret) = config.client_secret {
        params.push(("client_secret", secret.clone()));
    }

    let response = match client.post(&config.token_url).form(&params).send().await {
        Ok(r) => r,
        Err(e) => {
            tracing::warn!(error = %e, "OAuth token refresh request failed");
            return None;
        }
    };

    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        tracing::warn!(
            status = %status,
            body = %body,
            "OAuth token refresh returned non-success status"
        );
        return None;
    }

    match response.json().await {
        Ok(v) => Some(v),
        Err(e) => {
            tracing::warn!(error = %e, "Failed to parse token refresh response");
            None
        }
    }
}

/// Store the new access token (with expiry) and any rotated refresh token.
///
/// Returns `false` when the reply has no access token or storing it fails.
async fn store_refreshed_tokens(
    store: &(dyn SecretsStore + Send + Sync),
    user_id: &str,
    config: &OAuthRefreshConfig,
    token_data: &serde_json::Value,
) -> bool {
    let new_access_token = match token_data.get("access_token").and_then(|v| v.as_str()) {
        Some(t) => t,
        None => {
            tracing::warn!("Token refresh response missing access_token field");
            return false;
        }
    };

    // Store the new access token with expiry
    let mut access_params =
        crate::secrets::CreateSecretParams::new(&config.secret_name, new_access_token);
    if let Some(ref provider) = config.provider {
        access_params = access_params.with_provider(provider);
    }
    if let Some(expires_in) = token_data.get("expires_in").and_then(|v| v.as_u64()) {
        let expires_at = chrono::Utc::now() + chrono::Duration::seconds(expires_in as i64);
        access_params = access_params.with_expiry(expires_at);
    }

    if let Err(e) = store.create(user_id, access_params).await {
        tracing::warn!(error = %e, "Failed to store refreshed access token");
        return false;
    }

    // Store rotated refresh token if the provider sent a new one
    if let Some(new_refresh) = token_data.get("refresh_token").and_then(|v| v.as_str()) {
        let mut refresh_params =
            crate::secrets::CreateSecretParams::new(refresh_secret_name(config), new_refresh);
        if let Some(ref provider) = config.provider {
            refresh_params = refresh_params.with_provider(provider);
        }
        if let Err(e) = store.create(user_id, refresh_params).await {
            tracing::warn!(error = %e, "Failed to store rotated refresh token");
        }
    }

    true
}

/// Pre-resolve credentials for all HTTP capability mappings.
///
/// Called once per tool execution (in async context, before spawn_blocking)
/// so that the synchronous WASM host function can inject credentials
/// without needing async access to the secrets store.
///
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
    let store = match store {
        Some(s) => s,
        None => return Vec::new(),
    };

    // Check if the access token needs refreshing before resolving credentials.
    // This runs once per tool execution, keeping the hot path (credential injection
    // inside WASM) synchronous and allocation-free.
    if let Some(config) = oauth_refresh
        && access_token_needs_refresh(store, user_id, config).await
    {
        tracing::debug!(
            secret_name = %config.secret_name,
            "Access token expired or near expiry, attempting refresh"
        );
        refresh_oauth_token(store, user_id, config).await;
    }

    let http_cap = match &capabilities.http {
        Some(cap) => cap,
        None => return Vec::new(),
    };

    if http_cap.credentials.is_empty() {
        return Vec::new();
    }

    let mut resolved = Vec::new();

    for mapping in http_cap.credentials.values() {
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
