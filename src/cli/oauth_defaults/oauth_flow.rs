//! OAuth flow helpers for URL construction, token exchange, and persistence.

use std::collections::HashMap;
use std::time::Duration;

use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use rand::RngCore;
use sha2::{Digest, Sha256};
use url::Url;

use crate::llm::oauth_helpers::OAuthCallbackError;
use crate::secrets::{CreateSecretParams, SecretsStore};

/// Response from the OAuth token exchange.
pub struct OAuthTokenResponse {
    pub access_token: String,
    pub refresh_token: Option<String>,
    pub expires_in: Option<u64>,
}

/// Result of building an OAuth 2.0 authorization URL.
pub struct OAuthUrlResult {
    /// The full authorization URL to redirect the user to.
    pub url: String,
    /// PKCE code verifier (must be sent with the token exchange request).
    pub code_verifier: Option<String>,
    /// Random state parameter for CSRF protection (must be validated in callback).
    pub state: String,
}

/// Build an OAuth 2.0 authorization URL with optional PKCE and CSRF state.
///
/// Returns an `OAuthUrlResult` containing the authorization URL, optional PKCE
/// code verifier, and a random `state` parameter for CSRF protection. The caller
/// must validate the `state` value in the callback before exchanging the code.
pub fn build_oauth_url(
    authorization_url: &str,
    client_id: &str,
    redirect_uri: &str,
    scopes: &[String],
    use_pkce: bool,
    extra_params: &HashMap<String, String>,
) -> Result<OAuthUrlResult, OAuthCallbackError> {
    let (code_verifier, code_challenge) = if use_pkce {
        let mut verifier_bytes = [0u8; 32];
        rand::rngs::OsRng.fill_bytes(&mut verifier_bytes);
        let verifier = URL_SAFE_NO_PAD.encode(verifier_bytes);

        let mut hasher = Sha256::new();
        hasher.update(verifier.as_bytes());
        let challenge = URL_SAFE_NO_PAD.encode(hasher.finalize());

        (Some(verifier), Some(challenge))
    } else {
        (None, None)
    };

    let mut state_bytes = [0u8; 32];
    rand::rngs::OsRng.fill_bytes(&mut state_bytes);
    let state = URL_SAFE_NO_PAD.encode(state_bytes);

    let mut auth_url = Url::parse(authorization_url).map_err(|e| {
        OAuthCallbackError::Io(format!(
            "Invalid OAuth authorization URL '{authorization_url}': {e}"
        ))
    })?;
    {
        let mut query = auth_url.query_pairs_mut();
        query.append_pair("client_id", client_id);
        query.append_pair("response_type", "code");
        query.append_pair("redirect_uri", redirect_uri);
        query.append_pair("state", &state);

        if !scopes.is_empty() {
            query.append_pair("scope", &scopes.join(" "));
        }

        if let Some(ref challenge) = code_challenge {
            query.append_pair("code_challenge", challenge);
            query.append_pair("code_challenge_method", "S256");
        }

        for (key, value) in extra_params {
            query.append_pair(key, value);
        }
    }

    Ok(OAuthUrlResult {
        url: auth_url.to_string(),
        code_verifier,
        state,
    })
}

pub(super) fn format_bounded_body(body: &str) -> String {
    let normalized = body.split_whitespace().collect::<Vec<_>>().join(" ");
    if normalized.is_empty() {
        return "<empty body>".to_string();
    }

    if normalized.len() <= 200 {
        return normalized;
    }

    let mut end = 200;
    while end > 0 && !normalized.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}...", &normalized[..end])
}

/// Exchange an OAuth authorization code for tokens.
///
/// POSTs to `token_url` with the authorization code and optional PKCE verifier.
/// If `client_secret` is provided, uses HTTP Basic auth; otherwise includes
/// `client_id` in the form body (for public clients).
pub async fn exchange_oauth_code(
    token_url: &str,
    client_id: &str,
    client_secret: Option<&str>,
    code: &str,
    redirect_uri: &str,
    code_verifier: Option<&str>,
    access_token_field: &str,
) -> Result<OAuthTokenResponse, OAuthCallbackError> {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(60))
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .map_err(|e| OAuthCallbackError::Io(format!("Failed to build HTTP client: {e}")))?;
    let mut token_params = vec![
        ("grant_type", "authorization_code".to_string()),
        ("code", code.to_string()),
        ("redirect_uri", redirect_uri.to_string()),
    ];

    if let Some(verifier) = code_verifier {
        token_params.push(("code_verifier", verifier.to_string()));
    }

    let mut request = client.post(token_url);

    if let Some(secret) = client_secret {
        request = request.basic_auth(client_id, Some(secret));
    } else {
        token_params.push(("client_id", client_id.to_string()));
    }

    let token_response = request
        .form(&token_params)
        .send()
        .await
        .map_err(|e| OAuthCallbackError::Io(format!("Token exchange request failed: {e}")))?;

    if !token_response.status().is_success() {
        let status = token_response.status();
        let body = token_response.text().await.unwrap_or_default();
        let preview = format_bounded_body(&body);
        return Err(OAuthCallbackError::Io(format!(
            "Token exchange failed: {status} - {preview}"
        )));
    }

    let token_data: serde_json::Value = token_response
        .json()
        .await
        .map_err(|e| OAuthCallbackError::Io(format!("Failed to parse token response: {e}")))?;

    let access_token = token_data
        .get(access_token_field)
        .and_then(|v| v.as_str())
        .ok_or_else(|| {
            let fields: Vec<&str> = token_data
                .as_object()
                .map(|o| o.keys().map(|k| k.as_str()).collect())
                .unwrap_or_default();
            OAuthCallbackError::Io(format!(
                "No '{access_token_field}' field in token response (fields present: {fields:?})"
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

/// Store OAuth tokens (access + refresh) in the secrets store.
///
/// Also stores the granted scopes as `{secret_name}_scopes` so that scope
/// expansion can be detected on subsequent activations.
#[expect(
    clippy::too_many_arguments,
    reason = "persists the full OAuth token bundle and scope metadata"
)]
pub async fn store_oauth_tokens(
    store: &(dyn SecretsStore + Send + Sync),
    user_id: &str,
    secret_name: &str,
    provider: Option<&str>,
    access_token: &str,
    refresh_token: Option<&str>,
    expires_in: Option<u64>,
    scopes: &[String],
) -> Result<(), OAuthCallbackError> {
    let mut params = CreateSecretParams::new(secret_name, access_token);

    if let Some(prov) = provider {
        params = params.with_provider(prov);
    }

    if let Some(secs) = expires_in {
        let expires_at = chrono::Utc::now() + chrono::Duration::seconds(secs as i64);
        params = params.with_expiry(expires_at);
    }

    store
        .create(user_id, params)
        .await
        .map_err(|e| OAuthCallbackError::Io(format!("Failed to save token: {e}")))?;

    if let Some(rt) = refresh_token {
        let refresh_name = format!("{secret_name}_refresh_token");
        let mut refresh_params = CreateSecretParams::new(&refresh_name, rt);
        if let Some(prov) = provider {
            refresh_params = refresh_params.with_provider(prov);
        }
        store
            .create(user_id, refresh_params)
            .await
            .map_err(|e| OAuthCallbackError::Io(format!("Failed to save refresh token: {e}")))?;
    }

    if !scopes.is_empty() {
        let scopes_name = format!("{secret_name}_scopes");
        let scopes_value = scopes.join(" ");
        let scopes_params = CreateSecretParams::new(&scopes_name, &scopes_value);
        store.create(user_id, scopes_params).await.map_err(|e| {
            OAuthCallbackError::Io(format!("Failed to save token scopes metadata: {e}"))
        })?;
    }

    Ok(())
}

/// Validate an OAuth token against a tool's validation endpoint.
pub async fn validate_oauth_token(
    token: &str,
    validation: &crate::tools::wasm::ValidationEndpointSchema,
) -> Result<(), OAuthCallbackError> {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(10))
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .map_err(|e| OAuthCallbackError::Io(format!("Failed to build HTTP client: {e}")))?;

    let request = match validation.method.to_uppercase().as_str() {
        "POST" => client.post(&validation.url),
        _ => client.get(&validation.url),
    };

    let mut request = request.header("Authorization", format!("Bearer {token}"));

    for (key, value) in &validation.headers {
        request = request.header(key, value);
    }

    let response = request
        .send()
        .await
        .map_err(|e| OAuthCallbackError::Io(format!("Validation request failed: {e}")))?;

    if response.status().as_u16() == validation.success_status {
        return Ok(());
    }

    let status = response.status();
    let body = response.text().await.unwrap_or_default();
    let truncated = format_bounded_body(&body);

    Err(OAuthCallbackError::Io(format!(
        "Token validation failed: HTTP {status} (expected {}): {truncated}",
        validation.success_status
    )))
}
