//! Core OAuth types: errors, discovery metadata, registration payloads,
//! tokens, and PKCE challenge generation.

use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use rand::RngCore;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

/// OAuth authorization error.
#[derive(Debug, thiserror::Error)]
pub enum AuthError {
    #[error("Server does not support OAuth authorization")]
    NotSupported,

    #[error("Failed to discover authorization endpoints: {0}")]
    DiscoveryFailed(String),

    #[error("Authorization denied by user")]
    AuthorizationDenied,

    #[error("Token exchange failed: {0}")]
    TokenExchangeFailed(String),

    #[error("Token expired and refresh failed: {0}")]
    RefreshFailed(String),

    #[error("No access token available")]
    NoToken,

    #[error("Timeout waiting for authorization callback")]
    Timeout,

    #[error("Could not bind to callback port")]
    PortUnavailable,

    #[error("HTTP error: {0}")]
    Http(String),

    #[error("Secrets error: {0}")]
    Secrets(String),
}

/// OAuth protected resource metadata.
/// Discovered from /.well-known/oauth-protected-resource.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProtectedResourceMetadata {
    /// The protected resource identifier.
    pub resource: String,

    /// Authorization servers that can issue tokens for this resource.
    #[serde(default)]
    pub authorization_servers: Vec<String>,

    /// Scopes supported by this resource.
    #[serde(default)]
    pub scopes_supported: Vec<String>,
}

/// OAuth authorization server metadata.
/// Discovered from /.well-known/oauth-authorization-server.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuthorizationServerMetadata {
    /// Authorization server issuer.
    pub issuer: String,

    /// Authorization endpoint URL.
    pub authorization_endpoint: String,

    /// Token endpoint URL.
    pub token_endpoint: String,

    /// Dynamic client registration endpoint (if DCR is supported).
    #[serde(default)]
    pub registration_endpoint: Option<String>,

    /// Supported response types.
    #[serde(default)]
    pub response_types_supported: Vec<String>,

    /// Supported grant types.
    #[serde(default)]
    pub grant_types_supported: Vec<String>,

    /// Supported code challenge methods.
    #[serde(default)]
    pub code_challenge_methods_supported: Vec<String>,

    /// Scopes supported by this server.
    #[serde(default)]
    pub scopes_supported: Vec<String>,
}

/// Dynamic Client Registration request.
#[derive(Debug, Clone, Serialize)]
pub struct ClientRegistrationRequest {
    /// Human-readable client name.
    pub client_name: String,

    /// Redirect URIs for OAuth callbacks.
    pub redirect_uris: Vec<String>,

    /// Grant types the client will use.
    pub grant_types: Vec<String>,

    /// Response types the client will use.
    pub response_types: Vec<String>,

    /// Token endpoint authentication method.
    pub token_endpoint_auth_method: String,
}

/// Dynamic Client Registration response.
#[derive(Debug, Clone, Deserialize)]
pub struct ClientRegistrationResponse {
    /// The assigned client ID.
    pub client_id: String,

    /// Client secret (if issued).
    #[serde(default)]
    pub client_secret: Option<String>,

    /// When the client secret expires (if applicable).
    #[serde(default)]
    pub client_secret_expires_at: Option<u64>,

    /// Registration access token for managing the registration.
    #[serde(default)]
    pub registration_access_token: Option<String>,

    /// Registration client URI for managing the registration.
    #[serde(default)]
    pub registration_client_uri: Option<String>,
}

/// Access token with optional refresh token and expiry.
#[derive(Debug, Clone)]
pub struct AccessToken {
    /// The access token value.
    pub access_token: String,

    /// Token type (usually "Bearer").
    pub token_type: String,

    /// Seconds until expiration (if provided).
    pub expires_in: Option<u64>,

    /// Refresh token for obtaining new access tokens.
    pub refresh_token: Option<String>,

    /// Scopes granted.
    pub scope: Option<String>,
}

/// Token response from the authorization server.
#[derive(Debug, Deserialize)]
pub(super) struct TokenResponse {
    pub(super) access_token: String,
    pub(super) token_type: String,
    pub(super) expires_in: Option<u64>,
    pub(super) refresh_token: Option<String>,
    pub(super) scope: Option<String>,
}

/// PKCE verifier and challenge pair.
#[derive(Debug, Clone)]
pub struct PkceChallenge {
    /// Code verifier (high-entropy random string).
    pub verifier: String,
    /// Code challenge (S256 hash of verifier).
    pub challenge: String,
}

impl PkceChallenge {
    /// Generate a new PKCE challenge pair.
    pub fn generate() -> Self {
        let mut verifier_bytes = [0u8; 32];
        rand::rngs::OsRng.fill_bytes(&mut verifier_bytes);
        let verifier = URL_SAFE_NO_PAD.encode(verifier_bytes);

        let mut hasher = Sha256::new();
        hasher.update(verifier.as_bytes());
        let challenge = URL_SAFE_NO_PAD.encode(hasher.finalize());

        Self {
            verifier,
            challenge,
        }
    }
}
