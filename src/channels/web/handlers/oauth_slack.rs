//! Slack relay OAuth callback handler and validation helpers.

use std::collections::HashMap;
use std::sync::Arc;

use axum::{
    extract::{Query, State},
    response::{IntoResponse, Response},
};

use crate::channels::relay::DEFAULT_RELAY_NAME;
use crate::channels::web::handlers::chat_auth::clear_auth_mode;
use crate::channels::web::server::GatewayState;
use crate::channels::web::types::SseEvent;

struct ValidatedSlackCallback {
    provider: String,
    state: String,
    stream_token: String,
    team_id: String,
}

pub async fn slack_relay_oauth_callback_handler(
    State(state): State<Arc<GatewayState>>,
    Query(params): Query<HashMap<String, String>>,
) -> impl IntoResponse {
    if !state.oauth_rate_limiter.check() {
        return relay_oauth_page(false);
    }

    let Some(callback) = validate_slack_callback_params(&params) else {
        return relay_oauth_page(false);
    };

    let ext_mgr = match state.extension_manager.as_ref() {
        Some(manager) => manager,
        None => return relay_oauth_page(false),
    };

    if !csrf_state_matches(ext_mgr.secrets().as_ref(), &state.user_id, &callback.state).await {
        return relay_oauth_page(false);
    }

    let result = complete_slack_relay_oauth(&state, &callback).await;
    let success = result.is_ok();
    if let Err(error) = result {
        tracing::error!(error = %error, "Slack relay OAuth callback failed");
    } else {
        clear_auth_mode(state.as_ref(), Some(DEFAULT_RELAY_NAME)).await;
    }

    state.sse.broadcast(SseEvent::AuthCompleted {
        extension_name: DEFAULT_RELAY_NAME.to_string(),
        success,
        message: if success {
            "Slack connected successfully!".to_string()
        } else {
            "Connection failed. Check server logs for details.".to_string()
        },
    });

    relay_oauth_page(success)
}

fn validate_slack_callback_params(
    params: &HashMap<String, String>,
) -> Option<ValidatedSlackCallback> {
    let stream_token = match params.get("stream_token") {
        Some(token) if !token.is_empty() && token.len() <= 2048 => token.clone(),
        _ => return None,
    };

    // Slack omits `team_id` for some callback shapes, so empty means
    // "no team context provided" rather than "invalid team identifier".
    let team_id = params.get("team_id").cloned().unwrap_or_default();
    if !is_valid_team_id(&team_id) {
        return None;
    }

    let provider = params
        .get("provider")
        .cloned()
        .unwrap_or_else(|| "slack".to_string());
    if provider != "slack" {
        return None;
    }

    let state = match params.get("state") {
        Some(state) if !state.is_empty() && state.len() <= 128 => state.clone(),
        _ => return None,
    };

    Some(ValidatedSlackCallback {
        provider,
        state,
        stream_token,
        team_id,
    })
}

fn is_valid_team_id(team_id: &str) -> bool {
    team_id.is_empty()
        || (team_id.len() <= 21
            && team_id.starts_with('T')
            && team_id[1..].chars().all(|c| c.is_ascii_alphanumeric()))
}

async fn csrf_state_matches(
    secrets: &(dyn crate::secrets::SecretsStore + Send + Sync),
    user_id: &str,
    state_param: &str,
) -> bool {
    let state_key = format!("relay:{}:oauth_state", DEFAULT_RELAY_NAME);
    let stored_state = match secrets.get_decrypted(user_id, &state_key).await {
        Ok(secret) => secret.expose().to_string(),
        Err(_) => return false,
    };

    if state_param != stored_state {
        return false;
    }

    secrets.delete(user_id, &state_key).await.is_ok()
}

async fn complete_slack_relay_oauth(
    state: &GatewayState,
    callback: &ValidatedSlackCallback,
) -> Result<(), String> {
    let ext_mgr = state
        .extension_manager
        .as_ref()
        .ok_or_else(|| "Extension manager not available".to_string())?;

    let token_key = format!("relay:{}:stream_token", DEFAULT_RELAY_NAME);
    if let Err(error) = ext_mgr.secrets().delete(&state.user_id, &token_key).await {
        tracing::warn!(
            user_id = %state.user_id,
            secret_name = %token_key,
            error = %error,
            "Failed to delete previous Slack relay stream token"
        );
    }
    ext_mgr
        .secrets()
        .create(
            &state.user_id,
            crate::secrets::CreateSecretParams {
                name: token_key,
                value: secrecy::SecretString::from(callback.stream_token.clone()),
                provider: Some(callback.provider.clone()),
                expires_at: None,
            },
        )
        .await
        .map_err(|e| format!("Failed to store stream token: {e}"))?;

    if let Some(ref store) = state.store {
        let team_id_key = format!("relay:{}:team_id", DEFAULT_RELAY_NAME);
        store
            .set_setting(
                &state.user_id,
                &team_id_key,
                &serde_json::json!(callback.team_id),
            )
            .await
            .map_err(|e| format!("Failed to store relay team ID: {e}"))?;
    }

    ext_mgr
        .activate_stored_relay(DEFAULT_RELAY_NAME)
        .await
        .map_err(|e| format!("Failed to activate relay channel: {e}"))?;

    Ok(())
}

fn relay_oauth_page(success: bool) -> Response {
    let html = crate::cli::oauth_defaults::landing_html("Slack", success);
    axum::response::Html(html).into_response()
}
