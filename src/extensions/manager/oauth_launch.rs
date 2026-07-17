//! Shared plan type and stateless helpers for the WASM OAuth launch branches.
//!
//! [`super::oauth_flow`] resolves credentials and builds an [`OAuthLaunchPlan`],
//! then delegates the mechanical work (error formatting, state rewriting,
//! callback completion, and SSE broadcasting) to the free functions here.

use std::sync::Arc;

use crate::cli::oauth_defaults;
use crate::secrets::SecretsStore;

/// Fully resolved inputs for launching a WASM OAuth flow.
///
/// Built once by [`super::ExtensionManager::start_wasm_oauth`] and consumed by
/// whichever launch branch (gateway or local TCP listener) applies, so both
/// branches share a single, validated set of values.
pub(super) struct OAuthLaunchPlan {
    pub(super) name: String,
    pub(super) display_name: String,
    pub(super) redirect_uri: String,
    pub(super) merged_scopes: Vec<String>,
    pub(super) client_id: String,
    pub(super) client_secret: Option<String>,
    pub(super) code_verifier: Option<String>,
    pub(super) expected_state: String,
    pub(super) auth_url: String,
    pub(super) token_url: String,
    pub(super) access_token_field: String,
    pub(super) secret_name: String,
    pub(super) provider: Option<String>,
    pub(super) validation_endpoint: Option<crate::tools::wasm::ValidationEndpointSchema>,
}

/// Build the user-facing error for a missing OAuth `client_id`, mentioning the
/// relevant env var and (for Google providers only) the build-time flag.
pub(super) fn missing_client_id_error(
    name: &str,
    auth: &crate::tools::wasm::AuthCapabilitySchema,
    oauth: &crate::tools::wasm::OAuthConfigSchema,
) -> String {
    let env_name = oauth
        .client_id_env
        .as_deref()
        .unwrap_or("the client_id env var");
    let mut msg = format!(
        "OAuth client_id not configured for '{}'. \
         Enter it in the Setup tab or set {} env var",
        name, env_name
    );
    // Only mention the Google-specific build flag for Google providers
    if auth.secret_name.to_lowercase().contains("google") {
        msg.push_str(", or build with AXINITE_GOOGLE_CLIENT_ID");
    }
    msg.push('.');
    msg
}

/// Rewrite the `state` query parameter of `auth_url` to the platform-routed
/// nonce when it differs from the raw nonce; otherwise return `auth_url` as-is.
pub(super) fn rewrite_state_for_platform(auth_url: &str, expected_state: &str) -> String {
    let platform_state = oauth_defaults::build_platform_state(expected_state);
    if platform_state == expected_state {
        return auth_url.to_string();
    }
    auth_url.replace(
        &format!("state={}", urlencoding::encode(expected_state)),
        &format!("state={}", urlencoding::encode(&platform_state)),
    )
}

/// Wait for the local OAuth callback, exchange the code, optionally validate the
/// token, and store the resulting tokens.
pub(super) async fn complete_local_oauth(
    listener: tokio::net::TcpListener,
    plan: &OAuthLaunchPlan,
    secrets: Arc<dyn SecretsStore + Send + Sync>,
    user_id: &str,
) -> Result<(), String> {
    let code = oauth_defaults::wait_for_callback(
        listener,
        "/callback",
        "code",
        &plan.display_name,
        Some(&plan.expected_state),
    )
    .await
    .map_err(|e| e.to_string())?;

    let token_response = oauth_defaults::exchange_oauth_code(
        &plan.token_url,
        &plan.client_id,
        plan.client_secret.as_deref(),
        &code,
        &plan.redirect_uri,
        plan.code_verifier.as_deref(),
        &plan.access_token_field,
    )
    .await
    .map_err(|e| e.to_string())?;

    // Validate the token before storing (catches wrong account, etc.)
    if let Some(ref validation) = plan.validation_endpoint {
        oauth_defaults::validate_oauth_token(&token_response.access_token, validation)
            .await
            .map_err(|e| e.to_string())?;
    }

    oauth_defaults::store_oauth_tokens(
        secrets.as_ref(),
        user_id,
        &plan.secret_name,
        plan.provider.as_deref(),
        &token_response.access_token,
        token_response.refresh_token.as_deref(),
        token_response.expires_in,
        &plan.merged_scopes,
    )
    .await
    .map_err(|e| e.to_string())
}

/// Log the outcome of a local OAuth flow and broadcast an SSE completion event.
pub(super) fn broadcast_auth_result(
    sse_sender: Option<tokio::sync::broadcast::Sender<crate::channels::web::types::SseEvent>>,
    ext_name: &str,
    display_name: &str,
    result: Result<(), String>,
) {
    let (success, message) = match &result {
        Ok(()) => {
            tracing::info!(tool = %ext_name, "OAuth completed successfully");
            (true, format!("{} authenticated successfully", display_name))
        }
        Err(e) => {
            tracing::warn!(tool = %ext_name, error = %e, "WASM tool OAuth failed");
            (
                false,
                format!("{} authentication failed: {}", display_name, e),
            )
        }
    };

    if let Some(sender) = sse_sender {
        let _ = sender.send(crate::channels::web::types::SseEvent::AuthCompleted {
            extension_name: ext_name.to_string(),
            success,
            message,
        });
    }
}
