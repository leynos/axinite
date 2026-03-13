//! OAuth callback handlers for the web gateway.

use std::collections::HashMap;
use std::sync::Arc;

use axum::{
    Router,
    extract::{Query, State},
    response::IntoResponse,
    routing::get,
};

use crate::channels::web::handlers::chat_auth::clear_auth_mode;
use crate::channels::web::handlers::oauth_slack::slack_relay_oauth_callback_handler;
use crate::channels::web::server::GatewayState;
use crate::channels::web::types::SseEvent;

pub fn public_routes() -> Router<Arc<GatewayState>> {
    Router::new()
        .route("/oauth/callback", get(oauth_callback_handler))
        .route(
            "/oauth/slack/callback",
            get(slack_relay_oauth_callback_handler),
        )
}

/// Return an OAuth error landing page response.
fn oauth_error_page(label: &str) -> axum::response::Response {
    let html = crate::cli::oauth_defaults::landing_html(label, false);
    axum::response::Html(html).into_response()
}

/// OAuth callback handler for the web gateway.
///
/// This is a PUBLIC route (no Bearer token required) because OAuth providers
/// redirect the user's browser here. The `state` query parameter correlates
/// the callback with a pending OAuth flow registered by `start_wasm_oauth()`.
///
/// Used on hosted instances where `IRONCLAW_OAUTH_CALLBACK_URL` points to
/// the gateway (e.g., `https://kind-deer.agent1.near.ai/oauth/callback`).
/// Local/desktop mode continues to use the TCP listener on port 9876.
pub async fn oauth_callback_handler(
    State(state): State<Arc<GatewayState>>,
    Query(params): Query<HashMap<String, String>>,
) -> impl IntoResponse {
    use crate::cli::oauth_defaults;

    // Check for error from OAuth provider (e.g., user denied consent)
    if let Some(error) = params.get("error") {
        let description = params
            .get("error_description")
            .cloned()
            .unwrap_or_else(|| error.clone());
        return oauth_error_page(&description);
    }

    let state_param = match params.get("state") {
        Some(s) if !s.is_empty() => s.clone(),
        _ => return oauth_error_page("IronClaw"),
    };

    let code = match params.get("code") {
        Some(c) if !c.is_empty() => c.clone(),
        _ => return oauth_error_page("IronClaw"),
    };

    // Look up the pending flow by CSRF state (atomic remove prevents replay)
    let ext_mgr = match state.extension_manager.as_ref() {
        Some(mgr) => mgr,
        None => return oauth_error_page("IronClaw"),
    };

    // Strip instance prefix from state for registry lookup.
    // Platform nginx sends `state=instance:nonce` but flows are keyed by nonce only.
    let lookup_key = oauth_defaults::strip_instance_prefix(&state_param);

    let flow = ext_mgr
        .pending_oauth_flows()
        .write()
        .await
        .remove(lookup_key);

    let flow = match flow {
        Some(f) => f,
        None => {
            tracing::warn!(
                state = %state_param,
                lookup_key = %lookup_key,
                "OAuth callback received with unknown or expired state"
            );
            return oauth_error_page("IronClaw");
        }
    };

    // Check flow expiry (5 minutes, matching TCP listener timeout)
    if flow.created_at.elapsed() > oauth_defaults::OAUTH_FLOW_EXPIRY {
        tracing::warn!(
            extension = %flow.extension_name,
            "OAuth flow expired"
        );
        return oauth_error_page(&flow.display_name);
    }

    let result = complete_gateway_oauth_flow(&flow, &code).await;

    let (success, message) = match &result {
        Ok(()) => (
            true,
            format!("{} authenticated successfully", flow.display_name),
        ),
        Err(e) => (
            false,
            format!("{} authentication failed: {}", flow.display_name, e),
        ),
    };

    match &result {
        Ok(()) => {
            clear_auth_mode(state.as_ref()).await;
            tracing::info!(
                extension = %flow.extension_name,
                "OAuth completed successfully via gateway callback"
            );
        }
        Err(e) => {
            tracing::warn!(
                extension = %flow.extension_name,
                error = %e,
                "OAuth failed via gateway callback"
            );
        }
    }

    // Broadcast SSE event to notify the web UI
    if let Some(ref sender) = flow.sse_sender {
        let _ = sender.send(SseEvent::AuthCompleted {
            extension_name: flow.extension_name,
            success,
            message,
        });
    }

    let html = oauth_defaults::landing_html(&flow.display_name, success);
    axum::response::Html(html).into_response()
}

async fn complete_gateway_oauth_flow(
    flow: &crate::cli::oauth_defaults::PendingOAuthFlow,
    code: &str,
) -> Result<(), String> {
    use crate::cli::oauth_defaults;

    let exchange_proxy_url = std::env::var("IRONCLAW_OAUTH_EXCHANGE_URL").ok();
    let token_response = if let Some(ref proxy_url) = exchange_proxy_url {
        let gateway_token = flow.gateway_token.as_deref().unwrap_or_default();
        oauth_defaults::exchange_via_proxy(
            proxy_url,
            gateway_token,
            code,
            &flow.redirect_uri,
            flow.code_verifier.as_deref(),
            &flow.access_token_field,
        )
        .await
        .map_err(|e| e.to_string())?
    } else {
        oauth_defaults::exchange_oauth_code(
            &flow.token_url,
            &flow.client_id,
            flow.client_secret.as_deref(),
            code,
            &flow.redirect_uri,
            flow.code_verifier.as_deref(),
            &flow.access_token_field,
        )
        .await
        .map_err(|e| e.to_string())?
    };

    if let Some(ref validation) = flow.validation_endpoint {
        oauth_defaults::validate_oauth_token(&token_response.access_token, validation)
            .await
            .map_err(|e| e.to_string())?;
    }

    oauth_defaults::store_oauth_tokens(
        flow.secrets.as_ref(),
        &flow.user_id,
        &flow.secret_name,
        flow.provider.as_deref(),
        &token_response.access_token,
        token_response.refresh_token.as_deref(),
        token_response.expires_in,
        &flow.scopes,
    )
    .await
    .map_err(|e| e.to_string())
}
