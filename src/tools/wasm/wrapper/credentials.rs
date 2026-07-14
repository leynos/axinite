//! Host-credential resolution for WASM tools, including transparent
//! OAuth token refresh before execution.

use super::http::reject_private_ip;
use super::store::ResolvedHostCredential;
use super::*;

/// Refresh an expired OAuth access token using the stored refresh token.
///
/// Posts to the provider's token endpoint with `grant_type=refresh_token`,
/// then stores the new access token (with expiry) and rotated refresh token
/// (if the provider returns one).
///
/// SSRF defense: `token_url` originates from a tool's capabilities JSON, so
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
    // SSRF defense: token_url comes from the tool's capabilities file.
    if !config.token_url.starts_with("https://") {
        tracing::warn!(
            token_url = %config.token_url,
            "OAuth token_url must use HTTPS, refusing token refresh"
        );
        return false;
    }
    if let Err(reason) = reject_private_ip(&config.token_url) {
        tracing::warn!(
            token_url = %config.token_url,
            reason = %reason,
            "OAuth token_url points to a private/internal IP, refusing token refresh"
        );
        return false;
    }

    let refresh_name = format!("{}_refresh_token", config.secret_name);
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

    let client = match reqwest::Client::builder()
        .timeout(Duration::from_secs(15))
        .redirect(reqwest::redirect::Policy::none())
        .build()
    {
        Ok(c) => c,
        Err(e) => {
            tracing::warn!(error = %e, "Failed to build HTTP client for token refresh");
            return false;
        }
    };

    let mut params = vec![
        ("grant_type", "refresh_token".to_string()),
        ("refresh_token", refresh_secret.expose().to_string()),
        ("client_id", config.client_id.clone()),
    ];
    if let Some(ref secret) = config.client_secret {
        params.push(("client_secret", secret.clone()));
    }

    let response = match client.post(&config.token_url).form(&params).send().await {
        Ok(r) => r,
        Err(e) => {
            tracing::warn!(error = %e, "OAuth token refresh request failed");
            return false;
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
        return false;
    }

    let token_data: serde_json::Value = match response.json().await {
        Ok(v) => v,
        Err(e) => {
            tracing::warn!(error = %e, "Failed to parse token refresh response");
            return false;
        }
    };

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
            crate::secrets::CreateSecretParams::new(&refresh_name, new_refresh);
        if let Some(ref provider) = config.provider {
            refresh_params = refresh_params.with_provider(provider);
        }
        if let Err(e) = store.create(user_id, refresh_params).await {
            tracing::warn!(error = %e, "Failed to store rotated refresh token");
        }
    }

    tracing::info!(
        secret_name = %config.secret_name,
        "OAuth access token refreshed successfully"
    );
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
    if let Some(config) = oauth_refresh {
        let needs_refresh = match store.get(user_id, &config.secret_name).await {
            Ok(secret) => match secret.expires_at {
                Some(expires_at) => {
                    let buffer = chrono::Duration::minutes(5);
                    expires_at - buffer < chrono::Utc::now()
                }
                // No expires_at means legacy token, don't try to refresh
                None => false,
            },
            // Expired error from store means we definitely need to refresh
            Err(crate::secrets::SecretError::Expired) => true,
            // Not found or other errors: skip refresh, let the normal flow handle it
            Err(_) => false,
        };

        if needs_refresh {
            tracing::debug!(
                secret_name = %config.secret_name,
                "Access token expired or near expiry, attempting refresh"
            );
            refresh_oauth_token(store, user_id, config).await;
        }
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
        // Skip UrlPath credentials, they're handled by placeholder substitution
        if matches!(
            mapping.location,
            crate::secrets::CredentialLocation::UrlPath { .. }
        ) {
            continue;
        }

        let secret = match store.get_decrypted(user_id, &mapping.secret_name).await {
            Ok(s) => s,
            Err(e) => {
                tracing::debug!(
                    secret_name = %mapping.secret_name,
                    error = %e,
                    "Could not resolve credential for WASM tool (auth may not be configured)"
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
            "Pre-resolved host credentials for WASM tool execution"
        );
    }

    resolved
}
