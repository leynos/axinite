//! OAuth redirect callback handler for extension authentication flows.

use std::collections::HashMap;

use axum::{
    extract::{Query, State},
    http::StatusCode,
    response::IntoResponse,
};

use super::state::RouterState;

/// OAuth callback handler for extension authentication.
///
/// Handles OAuth redirect callbacks at /oauth/callback?code=xxx&state=yyy.
/// This is used when authenticating MCP servers or WASM tool OAuth flows
/// via a tunnel URL (remote callback).
#[allow(dead_code)]
pub(super) async fn oauth_callback_handler(
    State(_state): State<RouterState>,
    Query(params): Query<HashMap<String, String>>,
) -> impl IntoResponse {
    let code = params.get("code").cloned().unwrap_or_default();
    let _state = params.get("state").cloned().unwrap_or_default();

    if code.is_empty() {
        let error = params
            .get("error")
            .cloned()
            .unwrap_or_else(|| "unknown".to_string());
        return (
            StatusCode::BAD_REQUEST,
            axum::response::Html(format!(
                "<!DOCTYPE html><html><body style=\"font-family: sans-serif; \
                 display: flex; justify-content: center; align-items: center; \
                 height: 100vh; margin: 0; background: #191919; color: white;\">\
                 <div style=\"text-align: center;\">\
                 <h1>Authorization Failed</h1>\
                 <p>Error: {}</p>\
                 </div></body></html>",
                error
            )),
        );
    }

    // TODO: In a future iteration, use the state nonce to look up the pending auth
    // and complete the token exchange. For now, the OAuth flow uses local callbacks
    // via authorize_mcp_server() which handles the full flow synchronously.

    (
        StatusCode::OK,
        axum::response::Html(
            "<!DOCTYPE html><html><body style=\"font-family: sans-serif; \
             display: flex; justify-content: center; align-items: center; \
             height: 100vh; margin: 0; background: #191919; color: white;\">\
             <div style=\"text-align: center;\">\
             <h1>Connected!</h1>\
             <p>You can close this window and return to Axinite.</p>\
             </div></body></html>"
                .to_string(),
        ),
    )
}
