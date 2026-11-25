//! Generic webhook handler that validates secrets and signatures before
//! forwarding HTTP requests to the owning WASM channel.

use std::collections::HashMap;

use axum::{
    Json,
    body::Bytes,
    extract::{Path, Query, State},
    http::{HeaderMap, Method, StatusCode},
    response::IntoResponse,
};

use super::WasmChannelRouter;
use super::state::RouterState;

/// A rejection: the status and JSON body to send back.
type Reply = (StatusCode, Json<serde_json::Value>);

/// Build a rejection with a single `error` field.
fn reject(status: StatusCode, error: &str) -> Reply {
    (status, Json(serde_json::json!({ "error": error })))
}

/// Read a header as text, when it is present and valid UTF-8.
fn header_text<'a>(headers: &'a HeaderMap, name: &str) -> Option<&'a str> {
    headers.get(name).and_then(|v| v.to_str().ok())
}

/// Seconds since the Unix epoch, for signature freshness checks.
fn now_unix_secs() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}

/// Generic webhook handler that routes to the appropriate WASM channel.
pub(super) async fn webhook_handler(
    State(state): State<RouterState>,
    method: Method,
    Path(path): Path<String>,
    Query(query): Query<HashMap<String, String>>,
    headers: HeaderMap,
    body: Bytes,
) -> impl IntoResponse {
    let full_path = format!("/webhook/{}", path);

    tracing::info!(
        method = %method,
        path = %full_path,
        body_len = body.len(),
        "Webhook request received"
    );

    // Find the channel for this path
    let channel = match state.router.get_channel_for_path(&full_path).await {
        Some(c) => c,
        None => {
            tracing::warn!(
                path = %full_path,
                "No channel registered for webhook path"
            );
            return (
                StatusCode::NOT_FOUND,
                Json(serde_json::json!({
                    "error": "Channel not found for path",
                    "path": full_path
                })),
            );
        }
    };

    tracing::info!(
        channel = %channel.channel_name(),
        "Found channel for webhook"
    );

    let channel_name = channel.channel_name();

    if let Err(reply) = check_secret(&state.router, channel_name, &query, &headers).await {
        return reply;
    }
    if let Err(reply) = check_ed25519_signature(&state.router, channel_name, &headers, &body).await
    {
        return reply;
    }
    if let Err(reply) = check_hmac_signature(&state.router, channel_name, &headers, &body).await {
        return reply;
    }

    // Convert headers to HashMap
    let headers_map: HashMap<String, String> = headers
        .iter()
        .filter_map(|(k, v)| {
            v.to_str()
                .ok()
                .map(|v| (k.as_str().to_string(), v.to_string()))
        })
        .collect();

    // Call the WASM channel
    let secret_validated = state.router.requires_secret(channel_name).await;

    tracing::info!(
        channel = %channel_name,
        secret_validated = secret_validated,
        "Calling WASM channel on_http_request"
    );

    match channel
        .call_on_http_request(
            method.as_str(),
            &full_path,
            &headers_map,
            &query,
            &body,
            secret_validated,
        )
        .await
    {
        Ok(response) => channel_reply(channel_name, &response),
        Err(e) => {
            tracing::error!(
                channel = %channel_name,
                error = %e,
                "WASM channel callback failed"
            );
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({
                    "error": "Channel callback failed",
                    "details": e.to_string()
                })),
            )
        }
    }
}

/// Find the secret the caller sent: the `secret` query parameter, else the
/// channel's configured header, else the generic `X-Webhook-Secret` header.
fn provided_secret(
    query: &HashMap<String, String>,
    headers: &HeaderMap,
    secret_header_name: &str,
) -> Option<String> {
    query
        .get("secret")
        .cloned()
        .or_else(|| header_text(headers, secret_header_name).map(str::to_string))
        .or_else(|| {
            // Fallback to generic header if different from configured
            (secret_header_name != "X-Webhook-Secret")
                .then(|| header_text(headers, "X-Webhook-Secret").map(str::to_string))
                .flatten()
        })
}

/// Reject the request when the channel requires a secret and it is missing or wrong.
async fn check_secret(
    router: &WasmChannelRouter,
    channel_name: &str,
    query: &HashMap<String, String>,
    headers: &HeaderMap,
) -> Result<(), Reply> {
    if !router.requires_secret(channel_name).await {
        return Ok(());
    }
    // Get the secret header name for this channel (from capabilities or default)
    let secret_header_name = router.get_secret_header(channel_name).await;
    let provided = provided_secret(query, headers, &secret_header_name);

    tracing::debug!(
        channel = %channel_name,
        has_provided_secret = provided.is_some(),
        provided_secret_len = provided.as_ref().map(|s| s.len()),
        "Checking webhook secret"
    );

    let Some(secret) = provided else {
        tracing::warn!(
            channel = %channel_name,
            "Webhook secret required but not provided"
        );
        return Err(reject(StatusCode::UNAUTHORIZED, "Webhook secret required"));
    };
    if !router.validate_secret(channel_name, &secret).await {
        tracing::warn!(
            channel = %channel_name,
            "Webhook secret validation failed"
        );
        return Err(reject(StatusCode::UNAUTHORIZED, "Invalid webhook secret"));
    }
    tracing::debug!(channel = %channel_name, "Webhook secret validated");
    Ok(())
}

/// Ed25519 signature verification (Discord-style), when the channel registered a key.
async fn check_ed25519_signature(
    router: &WasmChannelRouter,
    channel_name: &str,
    headers: &HeaderMap,
    body: &Bytes,
) -> Result<(), Reply> {
    let Some(pub_key_hex) = router.get_signature_key(channel_name).await else {
        return Ok(());
    };
    let (Some(sig), Some(ts)) = (
        header_text(headers, "x-signature-ed25519"),
        header_text(headers, "x-signature-timestamp"),
    ) else {
        tracing::warn!(
            channel = %channel_name,
            "Signature headers missing but key is registered"
        );
        return Err(reject(
            StatusCode::UNAUTHORIZED,
            "Missing signature headers",
        ));
    };
    if !crate::channels::wasm::signature::verify_discord_signature(
        &pub_key_hex,
        sig,
        ts,
        body,
        now_unix_secs(),
    ) {
        tracing::warn!(
            channel = %channel_name,
            "Ed25519 signature verification failed"
        );
        return Err(reject(StatusCode::UNAUTHORIZED, "Invalid signature"));
    }
    tracing::debug!(channel = %channel_name, "Ed25519 signature verified");
    Ok(())
}

/// HMAC-SHA256 signature verification (Slack-style), when the channel registered a secret.
async fn check_hmac_signature(
    router: &WasmChannelRouter,
    channel_name: &str,
    headers: &HeaderMap,
    body: &Bytes,
) -> Result<(), Reply> {
    let Some(hmac_secret) = router.get_hmac_secret(channel_name).await else {
        return Ok(());
    };
    let (Some(ts), Some(sig)) = (
        header_text(headers, "x-slack-request-timestamp"),
        header_text(headers, "x-slack-signature"),
    ) else {
        tracing::warn!(
            channel = %channel_name,
            "Slack signature headers missing but secret is registered"
        );
        return Err(reject(
            StatusCode::UNAUTHORIZED,
            "Missing Slack signature headers",
        ));
    };
    if !crate::channels::wasm::signature::verify_slack_signature(
        &hmac_secret,
        ts,
        body,
        sig,
        now_unix_secs(),
    ) {
        tracing::warn!(
            channel = %channel_name,
            "HMAC-SHA256 signature verification failed"
        );
        return Err(reject(StatusCode::UNAUTHORIZED, "Invalid Slack signature"));
    }
    tracing::debug!(channel = %channel_name, "HMAC-SHA256 signature verified");
    Ok(())
}

/// Turn the channel's HTTP response into the reply, falling back to a raw-text
/// envelope when the body is not valid JSON.
fn channel_reply(channel_name: &str, response: &crate::channels::wasm::HttpResponse) -> Reply {
    let status = StatusCode::from_u16(response.status).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);

    tracing::info!(
        channel = %channel_name,
        status = %status,
        body_len = response.body.len(),
        "WASM channel on_http_request completed successfully"
    );

    let body_json: serde_json::Value = match serde_json::from_slice(&response.body) {
        Ok(value) => value,
        Err(_) => serde_json::json!({
            "raw": String::from_utf8_lossy(&response.body).to_string()
        }),
    };
    (status, Json(body_json))
}
