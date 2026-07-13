//! Token storage and lifecycle: persisting tokens and DCR client IDs in the
//! secrets store, checking authentication state, and refreshing access tokens.

use std::sync::Arc;
use std::time::Duration;

use crate::secrets::{CreateSecretParams, SecretsStore};
use crate::tools::mcp::config::McpServerConfig;

use super::discovery::discover_full_oauth_metadata;
use super::types::{AccessToken, AuthError, TokenResponse};
use super::url_safety::{canonical_resource_uri, validate_url_safe};

/// Store access and refresh tokens securely.
pub async fn store_tokens(
    secrets: &Arc<dyn SecretsStore + Send + Sync>,
    user_id: &str,
    server_config: &McpServerConfig,
    token: &AccessToken,
) -> Result<(), AuthError> {
    // Store access token
    let params = CreateSecretParams::new(server_config.token_secret_name(), &token.access_token)
        .with_provider(format!("mcp:{}", server_config.name));

    secrets
        .create(user_id, params)
        .await
        .map_err(|e| AuthError::Secrets(e.to_string()))?;

    // Store refresh token if present
    if let Some(ref refresh_token) = token.refresh_token {
        let params =
            CreateSecretParams::new(server_config.refresh_token_secret_name(), refresh_token)
                .with_provider(format!("mcp:{}", server_config.name));

        secrets
            .create(user_id, params)
            .await
            .map_err(|e| AuthError::Secrets(e.to_string()))?;
    }

    Ok(())
}

/// Store the DCR client ID for future token refresh.
pub async fn store_client_id(
    secrets: &Arc<dyn SecretsStore + Send + Sync>,
    user_id: &str,
    server_config: &McpServerConfig,
    client_id: &str,
) -> Result<(), AuthError> {
    let params = CreateSecretParams::new(server_config.client_id_secret_name(), client_id)
        .with_provider(format!("mcp:{}", server_config.name));

    secrets
        .create(user_id, params)
        .await
        .map(|_| ())
        .map_err(|e| AuthError::Secrets(e.to_string()))
}

/// Get the client ID for a server (from config or stored DCR).
async fn get_client_id(
    server_config: &McpServerConfig,
    secrets: &Arc<dyn SecretsStore + Send + Sync>,
    user_id: &str,
) -> Result<String, AuthError> {
    // First check if OAuth is configured with a client_id
    if let Some(ref oauth) = server_config.oauth {
        return Ok(oauth.client_id.clone());
    }

    // Otherwise try to get the DCR client_id from secrets
    match secrets
        .get_decrypted(user_id, &server_config.client_id_secret_name())
        .await
    {
        Ok(client_id) => Ok(client_id.expose().to_string()),
        Err(crate::secrets::SecretError::NotFound(_)) => Err(AuthError::RefreshFailed(
            "No client ID found. Please re-authenticate.".to_string(),
        )),
        Err(e) => Err(AuthError::Secrets(e.to_string())),
    }
}

/// Get the stored access token for an MCP server.
pub async fn get_access_token(
    server_config: &McpServerConfig,
    secrets: &Arc<dyn SecretsStore + Send + Sync>,
    user_id: &str,
) -> Result<Option<String>, AuthError> {
    match secrets
        .get_decrypted(user_id, &server_config.token_secret_name())
        .await
    {
        Ok(token) => Ok(Some(token.expose().to_string())),
        Err(crate::secrets::SecretError::NotFound(_)) => Ok(None),
        Err(e) => Err(AuthError::Secrets(e.to_string())),
    }
}

/// Check if a server has valid authentication.
///
/// Returns true if:
/// - A valid access token is stored (regardless of how it was obtained)
/// - The server doesn't require authentication at all
pub async fn is_authenticated(
    server_config: &McpServerConfig,
    secrets: &Arc<dyn SecretsStore + Send + Sync>,
    user_id: &str,
) -> bool {
    // Check if we have a stored token (from either pre-configured OAuth or DCR)
    secrets
        .exists(user_id, &server_config.token_secret_name())
        .await
        .unwrap_or(false)
}

/// Refresh an access token using the refresh token.
///
/// Works with both pre-configured OAuth and Dynamic Client Registration (DCR).
/// For DCR, retrieves the client_id from stored secrets.
pub async fn refresh_access_token(
    server_config: &McpServerConfig,
    secrets: &Arc<dyn SecretsStore + Send + Sync>,
    user_id: &str,
) -> Result<AccessToken, AuthError> {
    // Get client_id (from config or stored DCR)
    let client_id = get_client_id(server_config, secrets, user_id).await?;

    // Get the refresh token
    let refresh_token = secrets
        .get_decrypted(user_id, &server_config.refresh_token_secret_name())
        .await
        .map_err(|e| AuthError::RefreshFailed(format!("No refresh token: {}", e)))?;

    // Discover the token endpoint
    let token_url = if let Some(ref oauth) = server_config.oauth {
        if let Some(ref url) = oauth.token_url {
            url.clone()
        } else {
            // Discover from server
            let auth_meta = discover_full_oauth_metadata(&server_config.url).await?;
            auth_meta.token_endpoint
        }
    } else {
        // DCR - always discover
        let auth_meta = discover_full_oauth_metadata(&server_config.url).await?;
        auth_meta.token_endpoint
    };

    validate_url_safe(&token_url).await?;

    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(30))
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .map_err(|e| AuthError::Http(e.to_string()))?;

    // Compute canonical resource URI for RFC 8707
    let resource = canonical_resource_uri(&server_config.url);

    let params = vec![
        ("grant_type", "refresh_token".to_string()),
        ("refresh_token", refresh_token.expose().to_string()),
        ("client_id", client_id),
        ("resource", resource),
    ];

    let response = client
        .post(&token_url)
        .form(&params)
        .send()
        .await
        .map_err(|e| AuthError::RefreshFailed(e.to_string()))?;

    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        return Err(AuthError::RefreshFailed(format!(
            "HTTP {} - {}",
            status, body
        )));
    }

    let token_response: TokenResponse = response
        .json()
        .await
        .map_err(|e| AuthError::RefreshFailed(format!("Invalid response: {}", e)))?;

    let token = AccessToken {
        access_token: token_response.access_token,
        token_type: token_response.token_type,
        expires_in: token_response.expires_in,
        refresh_token: token_response.refresh_token,
        scope: token_response.scope,
    };

    // Store the new tokens
    store_tokens(secrets, user_id, server_config, &token).await?;

    Ok(token)
}
