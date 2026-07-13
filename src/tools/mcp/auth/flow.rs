//! The interactive OAuth 2.1 authorization flow: callback listener setup,
//! authorization URL construction, browser hand-off, callback handling, and
//! authorization-code token exchange.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use tokio::net::TcpListener;

use crate::cli::oauth_defaults::{self, OAUTH_CALLBACK_PORT};
use crate::secrets::SecretsStore;
use crate::tools::mcp::config::McpServerConfig;

use super::discovery::{discover_full_oauth_metadata, discover_oauth_endpoints, register_client};
use super::tokens::{store_client_id, store_tokens};
use super::types::{AccessToken, AuthError, PkceChallenge, TokenResponse};
use super::url_safety::{canonical_resource_uri, validate_url_safe};

/// Perform the OAuth 2.1 authorization flow for an MCP server.
///
/// Supports two modes:
/// 1. Pre-configured OAuth: Uses the client_id from server config
/// 2. Dynamic Client Registration: Discovers and registers with the server automatically
///
/// Flow:
/// 1. Discovers authorization endpoints from the server
/// 2. If no client_id configured, attempts Dynamic Client Registration (DCR)
/// 3. Generates PKCE challenge
/// 4. Opens browser for user authorization
/// 5. Receives callback with authorization code
/// 6. Exchanges code for access token
/// 7. Stores token securely
pub async fn authorize_mcp_server(
    server_config: &McpServerConfig,
    secrets: &Arc<dyn SecretsStore + Send + Sync>,
    user_id: &str,
) -> Result<AccessToken, AuthError> {
    // Find an available port for the callback first (needed for DCR)
    let (listener, port) = find_available_port().await?;
    let host = oauth_defaults::callback_host();
    let redirect_uri = format!("http://{}:{}/callback", host, port);

    // Warn when the callback is served over plain HTTP to a remote host.
    // Authorization codes travel unencrypted; SSH port forwarding is safer:
    //   ssh -L <port>:127.0.0.1:<port> user@your-server
    if !oauth_defaults::is_loopback_host(&host) {
        println!("Warning: MCP OAuth callback is using plain HTTP to a remote host ({host}).");
        println!("         Authorization codes will be transmitted unencrypted.");
        println!("         Consider SSH port forwarding instead:");
        println!("           ssh -L {port}:127.0.0.1:{port} user@{host}");
    }

    // Determine client_id and endpoints
    let (client_id, authorization_url, token_url, use_pkce, scopes, extra_params) =
        if let Some(oauth) = &server_config.oauth {
            // Pre-configured OAuth
            let (auth_url, tok_url) = discover_oauth_endpoints(server_config).await?;
            (
                oauth.client_id.clone(),
                auth_url,
                tok_url,
                oauth.use_pkce,
                oauth.scopes.clone(),
                oauth.extra_params.clone(),
            )
        } else {
            // Try Dynamic Client Registration
            println!("  Discovering OAuth endpoints...");
            let auth_meta = discover_full_oauth_metadata(&server_config.url).await?;

            let registration_endpoint = auth_meta
                .registration_endpoint
                .ok_or(AuthError::NotSupported)?;

            println!("  Registering client dynamically...");
            let registration = register_client(&registration_endpoint, &redirect_uri).await?;
            println!("  Client registered: {}", registration.client_id);

            (
                registration.client_id,
                auth_meta.authorization_endpoint,
                auth_meta.token_endpoint,
                true, // Always use PKCE for DCR clients
                auth_meta.scopes_supported,
                HashMap::new(),
            )
        };

    // Generate PKCE challenge
    let pkce = if use_pkce {
        Some(PkceChallenge::generate())
    } else {
        None
    };

    // Compute canonical resource URI for RFC 8707
    let resource = canonical_resource_uri(&server_config.url);

    // Validate the discovered authorization URL to prevent a malicious MCP server
    // from redirecting the user to a phishing page or non-HTTPS endpoint.
    validate_url_safe(&authorization_url)
        .await
        .map_err(|e| AuthError::DiscoveryFailed(format!("Unsafe authorization endpoint: {}", e)))?;

    // Build authorization URL
    let auth_url = build_authorization_url(
        &authorization_url,
        &client_id,
        &redirect_uri,
        &scopes,
        pkce.as_ref(),
        &extra_params,
        Some(&resource),
    );

    // Open browser
    println!("  Opening browser for {} login...", server_config.name);
    if let Err(e) = open::that(&auth_url) {
        println!("  Could not open browser: {}", e);
        println!("  Please open this URL manually:");
        println!("  {}", auth_url);
    }

    println!("  Waiting for authorization...");

    // Wait for callback
    let code = wait_for_authorization_callback(listener, &server_config.name).await?;

    println!("  Exchanging code for token...");

    // Exchange code for token
    let token = exchange_code_for_token(
        &token_url,
        &client_id,
        &code,
        &redirect_uri,
        pkce.as_ref(),
        Some(&resource),
    )
    .await?;

    // Store the tokens
    store_tokens(secrets, user_id, server_config, &token).await?;

    // Store the client_id for DCR (needed for token refresh)
    if server_config.oauth.is_none() {
        store_client_id(secrets, user_id, server_config, &client_id).await?;
    }

    Ok(token)
}

/// Bind the OAuth callback listener on the shared fixed port.
pub async fn find_available_port() -> Result<(TcpListener, u16), AuthError> {
    let listener = oauth_defaults::bind_callback_listener()
        .await
        .map_err(|_| AuthError::PortUnavailable)?;
    Ok((listener, OAUTH_CALLBACK_PORT))
}

/// Build the authorization URL with all required parameters.
pub fn build_authorization_url(
    base_url: &str,
    client_id: &str,
    redirect_uri: &str,
    scopes: &[String],
    pkce: Option<&PkceChallenge>,
    extra_params: &HashMap<String, String>,
    resource: Option<&str>,
) -> String {
    let mut url = format!(
        "{}?client_id={}&response_type=code&redirect_uri={}",
        base_url,
        urlencoding::encode(client_id),
        urlencoding::encode(redirect_uri)
    );

    if !scopes.is_empty() {
        url.push_str(&format!(
            "&scope={}",
            urlencoding::encode(&scopes.join(" "))
        ));
    }

    if let Some(pkce) = pkce {
        url.push_str(&format!(
            "&code_challenge={}&code_challenge_method=S256",
            pkce.challenge
        ));
    }

    for (key, value) in extra_params {
        url.push_str(&format!(
            "&{}={}",
            urlencoding::encode(key),
            urlencoding::encode(value)
        ));
    }

    if let Some(resource) = resource {
        url.push_str(&format!("&resource={}", urlencoding::encode(resource)));
    }

    url
}

/// Wait for the authorization callback and extract the code.
pub async fn wait_for_authorization_callback(
    listener: TcpListener,
    server_name: &str,
) -> Result<String, AuthError> {
    oauth_defaults::wait_for_callback(listener, "/callback", "code", server_name, None)
        .await
        .map_err(|e| match e {
            oauth_defaults::OAuthCallbackError::Denied => AuthError::AuthorizationDenied,
            oauth_defaults::OAuthCallbackError::Timeout => AuthError::Timeout,
            oauth_defaults::OAuthCallbackError::PortInUse(_, msg) => {
                AuthError::Http(format!("Port error: {}", msg))
            }
            oauth_defaults::OAuthCallbackError::StateMismatch { .. } => {
                AuthError::Http("CSRF state mismatch in OAuth callback".to_string())
            }
            oauth_defaults::OAuthCallbackError::Io(msg) => AuthError::Http(msg),
        })
}

/// Exchange the authorization code for an access token.
pub async fn exchange_code_for_token(
    token_url: &str,
    client_id: &str,
    code: &str,
    redirect_uri: &str,
    pkce: Option<&PkceChallenge>,
    resource: Option<&str>,
) -> Result<AccessToken, AuthError> {
    validate_url_safe(token_url).await?;

    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(30))
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .map_err(|e| AuthError::Http(e.to_string()))?;

    let mut params = vec![
        ("grant_type", "authorization_code".to_string()),
        ("code", code.to_string()),
        ("redirect_uri", redirect_uri.to_string()),
        ("client_id", client_id.to_string()),
    ];

    if let Some(pkce) = pkce {
        params.push(("code_verifier", pkce.verifier.clone()));
    }

    if let Some(resource) = resource {
        params.push(("resource", resource.to_string()));
    }

    let response = client
        .post(token_url)
        .form(&params)
        .send()
        .await
        .map_err(|e| AuthError::TokenExchangeFailed(e.to_string()))?;

    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        return Err(AuthError::TokenExchangeFailed(format!(
            "HTTP {} - {}",
            status, body
        )));
    }

    let token_response: TokenResponse = response
        .json()
        .await
        .map_err(|e| AuthError::TokenExchangeFailed(format!("Invalid response: {}", e)))?;

    Ok(AccessToken {
        access_token: token_response.access_token,
        token_type: token_response.token_type,
        expires_in: token_response.expires_in,
        refresh_token: token_response.refresh_token,
        scope: token_response.scope,
    })
}
