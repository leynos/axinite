//! OAuth endpoint discovery and Dynamic Client Registration.
//!
//! Implements the multi-strategy discovery chain (401 challenge, RFC 9728
//! protected resource metadata, direct authorization server metadata) plus
//! RFC 7591 dynamic client registration.

use std::time::Duration;

use crate::tools::mcp::config::McpServerConfig;

use super::types::{
    AuthError, AuthorizationServerMetadata, ClientRegistrationRequest, ClientRegistrationResponse,
    ProtectedResourceMetadata,
};
use super::url_safety::{build_well_known_uri, validate_url_safe};

// ---------------------------------------------------------------------------
// Multi-strategy OAuth discovery helpers
// ---------------------------------------------------------------------------

/// Parse the resource_metadata URL from a WWW-Authenticate header value.
///
/// Tries comma-separated parameters first, then whitespace-separated tokens
/// (e.g. `Bearer resource_metadata="url"`). A malformed match in the comma
/// pass stops the search without falling through to the whitespace pass,
/// matching the original behaviour.
pub(super) fn parse_resource_metadata_url(www_authenticate: &str) -> Option<String> {
    if let Some(value) = find_metadata_param(www_authenticate.split(','), false) {
        return value;
    }
    find_metadata_param(www_authenticate.split_whitespace(), true).flatten()
}

/// Scan header tokens for the first `resource_metadata=` parameter.
///
/// Returns `None` when no token matches, and the (possibly malformed, hence
/// inner `None`) parsed value of the first matching token otherwise.
fn find_metadata_param<'a>(
    mut parts: impl Iterator<Item = &'a str>,
    from_whitespace_split: bool,
) -> Option<Option<String>> {
    parts.find_map(|part| metadata_param_value(part.trim(), from_whitespace_split))
}

/// Extract the value from a single `resource_metadata=` header token.
///
/// Returns `None` when the token is not a resource_metadata parameter, and
/// `Some(None)` when it is one but the quoting is malformed (search stops).
/// Whitespace-split tokens may carry a trailing comma, which is stripped.
fn metadata_param_value(part: &str, from_whitespace_split: bool) -> Option<Option<String>> {
    if let Some(rest) = part.strip_prefix("resource_metadata=\"") {
        let rest = if from_whitespace_split {
            rest.trim_end_matches(',')
        } else {
            rest
        };
        return Some(rest.strip_suffix('"').map(|s| s.to_string()));
    }
    if let Some(rest) = part.strip_prefix("resource_metadata=") {
        let val = if from_whitespace_split {
            rest.trim_matches('"').trim_end_matches(',')
        } else {
            rest.trim_matches('"')
        };
        return Some(Some(val.to_string()));
    }
    None
}

/// Build a reqwest client with the given timeout and redirects disabled.
pub(super) fn build_no_redirect_client(timeout: Duration) -> Result<reqwest::Client, AuthError> {
    reqwest::Client::builder()
        .timeout(timeout)
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .map_err(|e| AuthError::Http(e.to_string()))
}

/// Fetch and deserialize a JSON metadata document from a validated URL.
///
/// `error_for_status` maps a non-success HTTP status to the caller's error,
/// letting each discovery strategy keep its own failure semantics.
async fn fetch_json_metadata<T: serde::de::DeserializeOwned>(
    url: &str,
    error_for_status: fn(reqwest::StatusCode) -> AuthError,
) -> Result<T, AuthError> {
    validate_url_safe(url).await?;

    let client = build_no_redirect_client(Duration::from_secs(10))?;

    let response = client
        .get(url)
        .send()
        .await
        .map_err(|e| AuthError::DiscoveryFailed(e.to_string()))?;

    if !response.status().is_success() {
        return Err(error_for_status(response.status()));
    }

    response
        .json()
        .await
        .map_err(|e| AuthError::DiscoveryFailed(format!("Invalid metadata: {}", e)))
}

/// Fetch protected resource metadata from a URL.
async fn fetch_resource_metadata(url: &str) -> Result<ProtectedResourceMetadata, AuthError> {
    fetch_json_metadata(url, |status| {
        AuthError::DiscoveryFailed(format!("HTTP {}", status))
    })
    .await
}

/// Try to discover OAuth metadata via 401 challenge response.
async fn discover_via_401(server_url: &str) -> Result<AuthorizationServerMetadata, AuthError> {
    validate_url_safe(server_url).await?;

    let client = build_no_redirect_client(Duration::from_secs(10))?;

    let response = client
        .post(server_url)
        .header("Content-Type", "application/json")
        .body("{}")
        .send()
        .await
        .map_err(|e| AuthError::DiscoveryFailed(e.to_string()))?;

    if response.status().as_u16() != 401 {
        return Err(AuthError::DiscoveryFailed(format!(
            "Expected 401, got {}",
            response.status()
        )));
    }

    let www_auth = response
        .headers()
        .get("WWW-Authenticate")
        .and_then(|v| v.to_str().ok())
        .ok_or_else(|| {
            AuthError::DiscoveryFailed("No WWW-Authenticate header in 401 response".to_string())
        })?;

    let resource_metadata_url = parse_resource_metadata_url(www_auth).ok_or_else(|| {
        AuthError::DiscoveryFailed(
            "No resource_metadata URL in WWW-Authenticate header".to_string(),
        )
    })?;

    let resource_meta = fetch_resource_metadata(&resource_metadata_url).await?;
    try_discover_from_auth_servers(&resource_meta).await
}

/// Try to discover auth server metadata from resource metadata's authorization_servers list.
async fn try_discover_from_auth_servers(
    resource_meta: &ProtectedResourceMetadata,
) -> Result<AuthorizationServerMetadata, AuthError> {
    let auth_server_url = resource_meta
        .authorization_servers
        .first()
        .ok_or_else(|| AuthError::DiscoveryFailed("No authorization servers listed".to_string()))?;

    discover_authorization_server(auth_server_url).await
}

// ---------------------------------------------------------------------------
// Discovery functions
// ---------------------------------------------------------------------------

/// Discover protected resource metadata from an MCP server.
pub async fn discover_protected_resource(
    server_url: &str,
) -> Result<ProtectedResourceMetadata, AuthError> {
    validate_url_safe(server_url).await?;
    let well_known_url = build_well_known_uri(server_url, "oauth-protected-resource")?;
    fetch_json_metadata(&well_known_url, |_| AuthError::NotSupported).await
}

/// Discover authorization server metadata.
pub async fn discover_authorization_server(
    auth_server_url: &str,
) -> Result<AuthorizationServerMetadata, AuthError> {
    validate_url_safe(auth_server_url).await?;
    let well_known_url = build_well_known_uri(auth_server_url, "oauth-authorization-server")?;
    fetch_json_metadata(&well_known_url, |status| {
        AuthError::DiscoveryFailed(format!("HTTP {}", status))
    })
    .await
}

/// Discover OAuth endpoints for an MCP server.
///
/// First checks if endpoints are explicitly configured, then falls back to discovery.
pub async fn discover_oauth_endpoints(
    server_config: &McpServerConfig,
) -> Result<(String, String), AuthError> {
    let oauth = server_config
        .oauth
        .as_ref()
        .ok_or(AuthError::NotSupported)?;

    // If endpoints are explicitly configured, use them
    if let (Some(auth_url), Some(token_url)) = (&oauth.authorization_url, &oauth.token_url) {
        return Ok((auth_url.clone(), token_url.clone()));
    }

    // Try to discover from the server
    let resource_meta = discover_protected_resource(&server_config.url).await?;

    // Get the first authorization server
    let auth_server_url = resource_meta
        .authorization_servers
        .first()
        .ok_or_else(|| AuthError::DiscoveryFailed("No authorization servers listed".to_string()))?;

    // Discover the authorization server metadata
    let auth_meta = discover_authorization_server(auth_server_url).await?;

    Ok((auth_meta.authorization_endpoint, auth_meta.token_endpoint))
}

/// Discover full OAuth metadata including DCR support.
///
/// Returns authorization server metadata which includes registration_endpoint if DCR is supported.
/// Uses a 3-strategy discovery chain:
/// 1. **401-based**: POST to MCP server, parse WWW-Authenticate header for resource_metadata URL
/// 2. **RFC 9728**: Discover protected resource metadata, then authorization server from it
/// 3. **Direct**: Treat MCP server as its own auth server
pub async fn discover_full_oauth_metadata(
    server_url: &str,
) -> Result<AuthorizationServerMetadata, AuthError> {
    // Strategy 1: 401-based discovery
    if let Ok(meta) = discover_via_401(server_url).await {
        return Ok(meta);
    }

    // Strategy 2: RFC 9728 protected resource discovery
    if let Ok(resource_meta) = discover_protected_resource(server_url).await
        && let Ok(meta) = try_discover_from_auth_servers(&resource_meta).await
    {
        return Ok(meta);
    }

    // Strategy 3: Direct - treat MCP server as its own auth server
    discover_authorization_server(server_url).await
}

/// Perform Dynamic Client Registration with an authorization server.
///
/// This allows clients to register themselves at runtime without pre-configured credentials.
pub async fn register_client(
    registration_endpoint: &str,
    redirect_uri: &str,
) -> Result<ClientRegistrationResponse, AuthError> {
    validate_url_safe(registration_endpoint).await?;

    let client = build_no_redirect_client(Duration::from_secs(30))?;

    let request = ClientRegistrationRequest {
        client_name: "IronClaw".to_string(),
        redirect_uris: vec![redirect_uri.to_string()],
        grant_types: vec![
            "authorization_code".to_string(),
            "refresh_token".to_string(),
        ],
        response_types: vec!["code".to_string()],
        token_endpoint_auth_method: "none".to_string(), // Public client (no secret)
    };

    let response = client
        .post(registration_endpoint)
        .json(&request)
        .send()
        .await
        .map_err(|e| AuthError::DiscoveryFailed(format!("DCR request failed: {}", e)))?;

    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        return Err(AuthError::DiscoveryFailed(format!(
            "DCR failed: HTTP {} - {}",
            status, body
        )));
    }

    response
        .json()
        .await
        .map_err(|e| AuthError::DiscoveryFailed(format!("Invalid DCR response: {}", e)))
}
