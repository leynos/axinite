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

use super::state::RouterState;

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

    // Check if secret is required
    if state.router.requires_secret(channel_name).await {
        // Get the secret header name for this channel (from capabilities or default)
        let secret_header_name = state.router.get_secret_header(channel_name).await;

        // Try to get secret from query param or the channel's configured header
        let provided_secret = query
            .get("secret")
            .cloned()
            .or_else(|| {
                headers
                    .get(&secret_header_name)
                    .and_then(|v| v.to_str().ok())
                    .map(|s| s.to_string())
            })
            .or_else(|| {
                // Fallback to generic header if different from configured
                if secret_header_name != "X-Webhook-Secret" {
                    headers
                        .get("X-Webhook-Secret")
                        .and_then(|v| v.to_str().ok())
                        .map(|s| s.to_string())
                } else {
                    None
                }
            });

        tracing::debug!(
            channel = %channel_name,
            has_provided_secret = provided_secret.is_some(),
            provided_secret_len = provided_secret.as_ref().map(|s| s.len()),
            "Checking webhook secret"
        );

        match provided_secret {
            Some(secret) => {
                if !state.router.validate_secret(channel_name, &secret).await {
                    tracing::warn!(
                        channel = %channel_name,
                        "Webhook secret validation failed"
                    );
                    return (
                        StatusCode::UNAUTHORIZED,
                        Json(serde_json::json!({
                            "error": "Invalid webhook secret"
                        })),
                    );
                }
                tracing::debug!(channel = %channel_name, "Webhook secret validated");
            }
            None => {
                tracing::warn!(
                    channel = %channel_name,
                    "Webhook secret required but not provided"
                );
                return (
                    StatusCode::UNAUTHORIZED,
                    Json(serde_json::json!({
                        "error": "Webhook secret required"
                    })),
                );
            }
        }
    }

    // Ed25519 signature verification (Discord-style)
    if let Some(pub_key_hex) = state.router.get_signature_key(channel_name).await {
        let sig_hex = headers
            .get("x-signature-ed25519")
            .and_then(|v| v.to_str().ok());
        let timestamp = headers
            .get("x-signature-timestamp")
            .and_then(|v| v.to_str().ok());

        match (sig_hex, timestamp) {
            (Some(sig), Some(ts)) => {
                let now_secs = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_secs() as i64;

                if !crate::channels::wasm::signature::verify_discord_signature(
                    &pub_key_hex,
                    sig,
                    ts,
                    &body,
                    now_secs,
                ) {
                    tracing::warn!(
                        channel = %channel_name,
                        "Ed25519 signature verification failed"
                    );
                    return (
                        StatusCode::UNAUTHORIZED,
                        Json(serde_json::json!({
                            "error": "Invalid signature"
                        })),
                    );
                }
                tracing::debug!(channel = %channel_name, "Ed25519 signature verified");
            }
            _ => {
                tracing::warn!(
                    channel = %channel_name,
                    "Signature headers missing but key is registered"
                );
                return (
                    StatusCode::UNAUTHORIZED,
                    Json(serde_json::json!({
                        "error": "Missing signature headers"
                    })),
                );
            }
        }
    }

    // HMAC-SHA256 signature verification (Slack-style)
    if let Some(hmac_secret) = state.router.get_hmac_secret(channel_name).await {
        let timestamp = headers
            .get("x-slack-request-timestamp")
            .and_then(|v| v.to_str().ok());
        let sig_header = headers
            .get("x-slack-signature")
            .and_then(|v| v.to_str().ok());

        match (timestamp, sig_header) {
            (Some(ts), Some(sig)) => {
                let now_secs = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_secs() as i64;

                if !crate::channels::wasm::signature::verify_slack_signature(
                    &hmac_secret,
                    ts,
                    &body,
                    sig,
                    now_secs,
                ) {
                    tracing::warn!(
                        channel = %channel_name,
                        "HMAC-SHA256 signature verification failed"
                    );
                    return (
                        StatusCode::UNAUTHORIZED,
                        Json(serde_json::json!({
                            "error": "Invalid Slack signature"
                        })),
                    );
                }
                tracing::debug!(channel = %channel_name, "HMAC-SHA256 signature verified");
            }
            _ => {
                tracing::warn!(
                    channel = %channel_name,
                    "Slack signature headers missing but secret is registered"
                );
                return (
                    StatusCode::UNAUTHORIZED,
                    Json(serde_json::json!({
                        "error": "Missing Slack signature headers"
                    })),
                );
            }
        }
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
        Ok(response) => {
            let status =
                StatusCode::from_u16(response.status).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);

            tracing::info!(
                channel = %channel_name,
                status = %status,
                body_len = response.body.len(),
                "WASM channel on_http_request completed successfully"
            );

            // Build response with headers; fall back to a raw-text envelope
            // when the body is not valid JSON.
            let body_json: serde_json::Value = match serde_json::from_slice(&response.body) {
                Ok(value) => value,
                Err(_) => serde_json::json!({
                    "raw": String::from_utf8_lossy(&response.body).to_string()
                }),
            };

            (status, Json(body_json))
        }
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
