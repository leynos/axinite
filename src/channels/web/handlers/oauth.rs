//! OAuth callback handlers for the web gateway.

use std::collections::HashMap;
use std::sync::Arc;

use axum::{
    Router,
    extract::{Query, State},
    response::IntoResponse,
    routing::get,
};

use crate::channels::relay::DEFAULT_RELAY_NAME;
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

    // Exchange the authorization code for tokens.
    // Use the platform exchange proxy when configured (keeps client_secret off container),
    // otherwise call the provider's token URL directly.
    let exchange_proxy_url = std::env::var("IRONCLAW_OAUTH_EXCHANGE_URL").ok();

    let result: Result<(), String> = async {
        let token_response = if let Some(ref proxy_url) = exchange_proxy_url {
            let gateway_token = flow.gateway_token.as_deref().unwrap_or_default();
            oauth_defaults::exchange_via_proxy(
                proxy_url,
                gateway_token,
                &code,
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
                &code,
                &flow.redirect_uri,
                flow.code_verifier.as_deref(),
                &flow.access_token_field,
            )
            .await
            .map_err(|e| e.to_string())?
        };

        // Validate the token before storing (catches wrong account, etc.)
        if let Some(ref validation) = flow.validation_endpoint {
            oauth_defaults::validate_oauth_token(&token_response.access_token, validation)
                .await
                .map_err(|e| e.to_string())?;
        }

        // Store tokens encrypted in the secrets store
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
        .map_err(|e| e.to_string())?;

        Ok(())
    }
    .await;

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

/// OAuth callback for Slack via channel-relay.
///
/// This is a PUBLIC route (no Bearer token required) because channel-relay
/// redirects the user's browser here after Slack OAuth completes.
/// Query params: `stream_token`, `provider`, `team_id`.
pub async fn slack_relay_oauth_callback_handler(
    State(state): State<Arc<GatewayState>>,
    Query(params): Query<HashMap<String, String>>,
) -> impl IntoResponse {
    // Rate limit
    if !state.oauth_rate_limiter.check() {
        return axum::response::Html(
            "<html><body style='font-family: system-ui; text-align: center; padding: 60px;'>\
             <h2>Too Many Requests</h2>\
             <p>Please try again later.</p>\
             </body></html>"
                .to_string(),
        )
        .into_response();
    }

    // Validate stream_token: required, non-empty, max 2048 bytes
    let stream_token = match params.get("stream_token") {
        Some(t) if !t.is_empty() && t.len() <= 2048 => t.clone(),
        Some(t) if t.len() > 2048 => {
            return axum::response::Html(
                "<html><body style='font-family: system-ui; text-align: center; padding: 60px;'>\
                 <h2>Error</h2><p>Invalid callback parameters.</p></body></html>"
                    .to_string(),
            )
            .into_response();
        }
        _ => {
            return axum::response::Html(
                "<html><body style='font-family: system-ui; text-align: center; padding: 60px;'>\
                 <h2>Error</h2><p>Invalid callback parameters.</p></body></html>"
                    .to_string(),
            )
            .into_response();
        }
    };

    // Validate team_id format: empty or T followed by alphanumeric (max 20 chars)
    let team_id = params.get("team_id").cloned().unwrap_or_default();
    if !team_id.is_empty() {
        let valid_team_id = team_id.len() <= 21
            && team_id.starts_with('T')
            && team_id[1..].chars().all(|c| c.is_ascii_alphanumeric());
        if !valid_team_id {
            return axum::response::Html(
                "<html><body style='font-family: system-ui; text-align: center; padding: 60px;'>\
                 <h2>Error</h2><p>Invalid callback parameters.</p></body></html>"
                    .to_string(),
            )
            .into_response();
        }
    }

    // Validate provider: must be "slack" (only supported provider)
    let provider = params
        .get("provider")
        .cloned()
        .unwrap_or_else(|| "slack".into());
    if provider != "slack" {
        return axum::response::Html(
            "<html><body style='font-family: system-ui; text-align: center; padding: 60px;'>\
             <h2>Error</h2><p>Invalid callback parameters.</p></body></html>"
                .to_string(),
        )
        .into_response();
    }

    let ext_mgr = match state.extension_manager.as_ref() {
        Some(mgr) => mgr,
        None => {
            return axum::response::Html(
                "<html><body style='font-family: system-ui; text-align: center; padding: 60px;'>\
                 <h2>Error</h2><p>Extension manager not available.</p></body></html>"
                    .to_string(),
            )
            .into_response();
        }
    };

    // Validate CSRF state parameter
    let state_param = match params.get("state") {
        Some(s) if !s.is_empty() && s.len() <= 128 => s.clone(),
        _ => {
            return axum::response::Html(
                "<html><body style='font-family: system-ui; text-align: center; padding: 60px;'>\
                 <h2>Error</h2><p>Invalid or expired authorization.</p></body></html>"
                    .to_string(),
            )
            .into_response();
        }
    };

    let state_key = format!("relay:{}:oauth_state", DEFAULT_RELAY_NAME);
    let stored_state = match ext_mgr
        .secrets()
        .get_decrypted(&state.user_id, &state_key)
        .await
    {
        Ok(secret) => secret.expose().to_string(),
        Err(_) => {
            return axum::response::Html(
                "<html><body style='font-family: system-ui; text-align: center; padding: 60px;'>\
                 <h2>Error</h2><p>Invalid or expired authorization.</p></body></html>"
                    .to_string(),
            )
            .into_response();
        }
    };

    if state_param != stored_state {
        return axum::response::Html(
            "<html><body style='font-family: system-ui; text-align: center; padding: 60px;'>\
             <h2>Error</h2><p>Invalid or expired authorization.</p></body></html>"
                .to_string(),
        )
        .into_response();
    }

    // Delete the nonce (one-time use)
    let _ = ext_mgr.secrets().delete(&state.user_id, &state_key).await;

    let result: Result<(), String> = async {
        // Store the stream token as a secret
        let token_key = format!("relay:{}:stream_token", DEFAULT_RELAY_NAME);
        let _ = ext_mgr.secrets().delete(&state.user_id, &token_key).await;
        ext_mgr
            .secrets()
            .create(
                &state.user_id,
                crate::secrets::CreateSecretParams {
                    name: token_key,
                    value: secrecy::SecretString::from(stream_token),
                    provider: Some(provider.clone()),
                    expires_at: None,
                },
            )
            .await
            .map_err(|e| format!("Failed to store stream token: {}", e))?;

        // Store team_id in settings
        if let Some(ref store) = state.store {
            let team_id_key = format!("relay:{}:team_id", DEFAULT_RELAY_NAME);
            let _ = store
                .set_setting(&state.user_id, &team_id_key, &serde_json::json!(team_id))
                .await;
        }

        // Activate the relay channel
        ext_mgr
            .activate_stored_relay(DEFAULT_RELAY_NAME)
            .await
            .map_err(|e| format!("Failed to activate relay channel: {}", e))?;

        Ok(())
    }
    .await;

    let (success, message) = match &result {
        Ok(()) => (true, "Slack connected successfully!".to_string()),
        Err(e) => {
            tracing::error!(error = %e, "Slack relay OAuth callback failed");
            (
                false,
                "Connection failed. Check server logs for details.".to_string(),
            )
        }
    };

    // Broadcast SSE event to notify the web UI
    state.sse.broadcast(SseEvent::AuthCompleted {
        extension_name: DEFAULT_RELAY_NAME.to_string(),
        success,
        message: message.clone(),
    });

    if success {
        axum::response::Html(
            "<html><body style='font-family: system-ui; text-align: center; padding: 60px;'>\
             <h2>Slack Connected!</h2>\
             <p>You can close this tab and return to IronClaw.</p>\
             <script>window.close()</script>\
             </body></html>"
                .to_string(),
        )
        .into_response()
    } else {
        axum::response::Html(format!(
            "<html><body style='font-family: system-ui; text-align: center; padding: 60px;'>\
             <h2>Connection Failed</h2>\
             <p>{}</p>\
             </body></html>",
            message
        ))
        .into_response()
    }
}
