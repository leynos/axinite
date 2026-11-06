//! Hosted gateway helpers for pending OAuth flows and proxy token exchange.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use tokio::sync::RwLock;

use super::oauth_flow::{OAuthTokenResponse, format_bounded_body};
use crate::llm::oauth_helpers::OAuthCallbackError;
use crate::secrets::SecretsStore;

/// State for an in-progress OAuth flow, keyed by CSRF `state` parameter.
///
/// Created by `start_wasm_oauth()` and consumed by the web gateway's
/// `/oauth/callback` handler when running in hosted mode.
pub struct PendingOAuthFlow {
    /// Name of the extension being authorized. Used to cancel flows for that extension.
    pub extension_name: String,
    /// Name shown on the landing page after the callback completes.
    pub display_name: String,
    /// Provider token endpoint that the authorization code is exchanged at.
    pub token_url: String,
    /// OAuth client identifier sent with the code exchange.
    pub client_id: String,
    /// OAuth client secret, when the provider uses one. `None` for PKCE-only clients.
    pub client_secret: Option<String>,
    /// Redirect URI that was sent with the authorization request. It must match on exchange.
    pub redirect_uri: String,
    /// PKCE code verifier paired with the challenge sent at authorization time.
    pub code_verifier: Option<String>,
    /// Name of the JSON field in the token response that holds the access token.
    pub access_token_field: String,
    /// Secret-store name the access token is saved under. The refresh token is saved
    /// as `{secret_name}_refresh_token`.
    pub secret_name: String,
    /// Provider hint saved alongside the tokens, such as `"notion"`.
    pub provider: Option<String>,
    /// Endpoint the new access token is checked against before it is stored.
    pub validation_endpoint: Option<crate::tools::wasm::ValidationEndpointSchema>,
    /// Scopes requested by the flow. Saved as `{secret_name}_scopes` when non-empty.
    pub scopes: Vec<String>,
    /// Owner the tokens are stored for.
    pub user_id: String,
    /// Secret store that receives the tokens once the exchange succeeds.
    pub secrets: Arc<dyn SecretsStore + Send + Sync>,
    /// Channel for notifying the web UI when the flow finishes. `None` when no UI is listening.
    pub sse_sender: Option<tokio::sync::broadcast::Sender<crate::channels::web::types::SseEvent>>,
    /// Bearer token for the hosted token-exchange proxy. Only used when the proxy is configured.
    pub gateway_token: Option<String>,
    /// When the flow was registered. Flows older than `OAUTH_FLOW_EXPIRY` are swept.
    pub created_at: std::time::Instant,
}

impl std::fmt::Debug for PendingOAuthFlow {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PendingOAuthFlow")
            .field("extension_name", &self.extension_name)
            .field("display_name", &self.display_name)
            .field("secret_name", &self.secret_name)
            .field("created_at", &self.created_at)
            .finish_non_exhaustive()
    }
}

/// Thread-safe registry of pending OAuth flows, keyed by CSRF `state` parameter.
pub type PendingOAuthRegistry = Arc<RwLock<HashMap<String, PendingOAuthFlow>>>;

/// Create a new empty pending OAuth flow registry.
pub fn new_pending_oauth_registry() -> PendingOAuthRegistry {
    Arc::new(RwLock::new(HashMap::new()))
}

/// Maximum age for pending OAuth flows (5 minutes, matching TCP listener timeout).
pub const OAUTH_FLOW_EXPIRY: Duration = Duration::from_secs(300);

/// Remove expired flows from the registry.
pub async fn sweep_expired_flows(registry: &PendingOAuthRegistry) {
    let mut flows = registry.write().await;
    flows.retain(|_, flow| flow.created_at.elapsed() < OAUTH_FLOW_EXPIRY);
}

/// Exchange an OAuth authorization code via the platform's token exchange proxy.
pub async fn exchange_via_proxy(
    proxy_url: &str,
    gateway_token: &str,
    code: &str,
    redirect_uri: &str,
    code_verifier: Option<&str>,
    access_token_field: &str,
) -> Result<OAuthTokenResponse, OAuthCallbackError> {
    if gateway_token.is_empty() {
        return Err(OAuthCallbackError::Io(
            "Gateway auth token is required for proxy token exchange".to_string(),
        ));
    }
    let exchange_url = format!("{}/oauth/exchange", proxy_url.trim_end_matches('/'));

    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(60))
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .map_err(|e| OAuthCallbackError::Io(format!("Failed to build HTTP client: {e}")))?;
    let mut params = vec![
        ("code", code.to_string()),
        ("redirect_uri", redirect_uri.to_string()),
    ];
    if let Some(verifier) = code_verifier {
        params.push(("code_verifier", verifier.to_string()));
    }

    let response = client
        .post(&exchange_url)
        .bearer_auth(gateway_token)
        .form(&params)
        .send()
        .await
        .map_err(|e| OAuthCallbackError::Io(format!("Token exchange proxy request failed: {e}")))?;

    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        let preview = format_bounded_body(&body);
        return Err(OAuthCallbackError::Io(format!(
            "Token exchange proxy failed: {status} - {preview}"
        )));
    }

    let token_data: serde_json::Value = response
        .json()
        .await
        .map_err(|e| OAuthCallbackError::Io(format!("Failed to parse proxy response: {e}")))?;

    let access_token = token_data
        .get(access_token_field)
        .and_then(|v| v.as_str())
        .ok_or_else(|| {
            let fields: Vec<&str> = token_data
                .as_object()
                .map(|o| o.keys().map(|k| k.as_str()).collect())
                .unwrap_or_default();
            OAuthCallbackError::Io(format!(
                "No '{access_token_field}' field in proxy response (fields present: {fields:?})"
            ))
        })?
        .to_string();

    let refresh_token = token_data
        .get("refresh_token")
        .and_then(|v| v.as_str())
        .map(String::from);
    let expires_in = token_data.get("expires_in").and_then(|v| v.as_u64());

    Ok(OAuthTokenResponse {
        access_token,
        refresh_token,
        expires_in,
    })
}
