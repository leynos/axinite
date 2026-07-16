//! Axum request handlers for the HTTP webhook channel.
//!
//! Contains the webhook request/response payload types, attachment
//! validation and decoding, rate limiting, secret checking, and the
//! message forwarding pipeline.

use std::sync::Arc;

use axum::{Json, extract::State, http::StatusCode, response::IntoResponse};
use secrecy::ExposeSecret;
use serde::{Deserialize, Serialize};
use subtle::ConstantTimeEq;
use tokio::sync::oneshot;
use uuid::Uuid;

use super::HttpChannelState;
use crate::channels::{AttachmentKind, IncomingAttachment, IncomingMessage};

/// Maximum number of pending wait-for-response requests.
const MAX_PENDING_RESPONSES: usize = 100;

/// Maximum requests per minute.
const MAX_REQUESTS_PER_MINUTE: u32 = 60;

/// Maximum content length for a single message.
const MAX_CONTENT_BYTES: usize = 32 * 1024;

#[derive(Debug, Deserialize)]
pub(super) struct WebhookRequest {
    /// User or client identifier (ignored, user is fixed by server config).
    #[serde(default)]
    user_id: Option<String>,
    /// Message content.
    content: String,
    /// Optional thread ID for conversation tracking.
    thread_id: Option<String>,
    /// Optional webhook secret for authentication.
    secret: Option<String>,
    /// Whether to wait for a synchronous response.
    #[serde(default)]
    wait_for_response: bool,
    /// Optional file attachments (base64-encoded).
    #[serde(default)]
    attachments: Vec<AttachmentData>,
}

/// A file attachment in a webhook request.
#[derive(Debug, Deserialize)]
pub(super) struct AttachmentData {
    /// MIME type (e.g. "image/png", "application/pdf").
    mime_type: String,
    /// Optional filename.
    #[serde(default)]
    filename: Option<String>,
    /// Base64-encoded file data.
    #[serde(default)]
    data_base64: Option<String>,
    /// URL to fetch the file from (not downloaded server-side for SSRF prevention).
    #[serde(default)]
    url: Option<String>,
}

/// Maximum size per attachment (5 MB decoded).
const MAX_ATTACHMENT_BYTES: usize = 5 * 1024 * 1024;
/// Maximum total attachment size (10 MB decoded).
const MAX_TOTAL_ATTACHMENT_BYTES: usize = 10 * 1024 * 1024;
/// Maximum number of attachments per request.
const MAX_ATTACHMENTS: usize = 5;

#[derive(Debug, Serialize)]
pub(super) struct WebhookResponse {
    /// Message ID assigned to this request.
    message_id: Uuid,
    /// Status of the request.
    status: String,
    /// Response content (only if wait_for_response was true).
    response: Option<String>,
}

#[derive(Debug, Serialize)]
pub(super) struct HealthResponse {
    status: String,
    channel: String,
}

pub(super) async fn health_handler() -> impl IntoResponse {
    Json(HealthResponse {
        status: "healthy".to_string(),
        channel: "http".to_string(),
    })
}

pub(super) async fn webhook_handler(
    State(state): State<Arc<HttpChannelState>>,
    Json(req): Json<WebhookRequest>,
) -> (StatusCode, Json<WebhookResponse>) {
    // Rate limiting
    {
        let mut limiter = state.rate_limit.lock().await;
        if limiter.window_start.elapsed() >= std::time::Duration::from_secs(60) {
            limiter.window_start = std::time::Instant::now();
            limiter.request_count = 0;
        }
        limiter.request_count += 1;
        if limiter.request_count > MAX_REQUESTS_PER_MINUTE {
            return (
                StatusCode::TOO_MANY_REQUESTS,
                Json(WebhookResponse {
                    message_id: Uuid::nil(),
                    status: "error".to_string(),
                    response: Some("Rate limit exceeded".to_string()),
                }),
            );
        }
    }

    let _ = req.user_id.as_ref().map(|user_id| {
        tracing::debug!(
            provided_user_id = %user_id,
            "HTTP webhook request provided user_id, ignoring in favour of configured user_id"
        );
    });

    // Validate secret if configured
    if let Some(ref expected_secret) = *state.webhook_secret.read().await {
        let expected_bytes = expected_secret.expose_secret().as_bytes();
        match &req.secret {
            Some(provided) if bool::from(provided.as_bytes().ct_eq(expected_bytes)) => {
                // Secret matches, continue
            }
            Some(_) => {
                return (
                    StatusCode::UNAUTHORIZED,
                    Json(WebhookResponse {
                        message_id: Uuid::nil(),
                        status: "error".to_string(),
                        response: Some("Invalid webhook secret".to_string()),
                    }),
                );
            }
            None => {
                return (
                    StatusCode::UNAUTHORIZED,
                    Json(WebhookResponse {
                        message_id: Uuid::nil(),
                        status: "error".to_string(),
                        response: Some("Webhook secret required".to_string()),
                    }),
                );
            }
        }
    }

    if req.content.len() > MAX_CONTENT_BYTES {
        return (
            StatusCode::PAYLOAD_TOO_LARGE,
            Json(WebhookResponse {
                message_id: Uuid::nil(),
                status: "error".to_string(),
                response: Some("Content too large".to_string()),
            }),
        );
    }

    // Validate and decode attachments
    let attachments = if !req.attachments.is_empty() {
        if req.attachments.len() > MAX_ATTACHMENTS {
            return (
                StatusCode::BAD_REQUEST,
                Json(WebhookResponse {
                    message_id: Uuid::nil(),
                    status: "error".to_string(),
                    response: Some(format!("Too many attachments (max {})", MAX_ATTACHMENTS)),
                }),
            );
        }

        let mut decoded_attachments = Vec::new();
        let mut total_bytes: usize = 0;
        for att in &req.attachments {
            if let Some(ref b64) = att.data_base64 {
                use base64::Engine;
                let data = match base64::engine::general_purpose::STANDARD.decode(b64) {
                    Ok(d) => d,
                    Err(_) => {
                        return (
                            StatusCode::BAD_REQUEST,
                            Json(WebhookResponse {
                                message_id: Uuid::nil(),
                                status: "error".to_string(),
                                response: Some("Invalid base64 in attachment".to_string()),
                            }),
                        );
                    }
                };
                if data.len() > MAX_ATTACHMENT_BYTES {
                    return (
                        StatusCode::PAYLOAD_TOO_LARGE,
                        Json(WebhookResponse {
                            message_id: Uuid::nil(),
                            status: "error".to_string(),
                            response: Some(format!(
                                "Attachment too large (max {} bytes)",
                                MAX_ATTACHMENT_BYTES
                            )),
                        }),
                    );
                }
                total_bytes += data.len();
                if total_bytes > MAX_TOTAL_ATTACHMENT_BYTES {
                    return (
                        StatusCode::PAYLOAD_TOO_LARGE,
                        Json(WebhookResponse {
                            message_id: Uuid::nil(),
                            status: "error".to_string(),
                            response: Some("Total attachment size exceeds limit".to_string()),
                        }),
                    );
                }
                decoded_attachments.push(IncomingAttachment {
                    id: Uuid::new_v4().to_string(),
                    kind: AttachmentKind::from_mime_type(&att.mime_type),
                    mime_type: att.mime_type.clone(),
                    filename: att.filename.clone(),
                    size_bytes: Some(data.len() as u64),
                    source_url: None,
                    storage_key: None,
                    extracted_text: None,
                    data,
                    duration_secs: None,
                });
            } else if let Some(ref url) = att.url {
                // URL-only attachment: set source_url but don't download (SSRF prevention)
                decoded_attachments.push(IncomingAttachment {
                    id: Uuid::new_v4().to_string(),
                    kind: AttachmentKind::from_mime_type(&att.mime_type),
                    mime_type: att.mime_type.clone(),
                    filename: att.filename.clone(),
                    size_bytes: None,
                    source_url: Some(url.clone()),
                    storage_key: None,
                    extracted_text: None,
                    data: Vec::new(),
                    duration_secs: None,
                });
            }
        }
        decoded_attachments
    } else {
        Vec::new()
    };

    let mut msg = IncomingMessage::new("http", &state.user_id, &req.content).with_metadata(
        serde_json::json!({
            "wait_for_response": req.wait_for_response,
        }),
    );

    if !attachments.is_empty() {
        msg = msg.with_attachments(attachments);
    }

    if let Some(thread_id) = &req.thread_id {
        msg = msg.with_thread(thread_id);
    }

    process_message(state, msg, req.wait_for_response).await
}

/// Build a webhook error response with the given status code and message.
fn error_response(
    msg_id: Uuid,
    code: StatusCode,
    text: &str,
) -> (StatusCode, Json<WebhookResponse>) {
    (
        code,
        Json(WebhookResponse {
            message_id: msg_id,
            status: "error".to_string(),
            response: Some(text.to_string()),
        }),
    )
}

/// Register a pending response channel for a synchronous request.
///
/// Fails with 429 when too many synchronous requests are already pending.
async fn register_response_channel(
    state: &HttpChannelState,
    msg_id: Uuid,
) -> Result<oneshot::Receiver<String>, (StatusCode, Json<WebhookResponse>)> {
    if state.pending_responses.read().await.len() >= MAX_PENDING_RESPONSES {
        return Err(error_response(
            msg_id,
            StatusCode::TOO_MANY_REQUESTS,
            "Too many pending requests",
        ));
    }

    let (tx, rx) = oneshot::channel();
    state.pending_responses.write().await.insert(msg_id, tx);
    Ok(rx)
}

/// Forward the message to the agent loop.
async fn forward_to_agent(
    state: &HttpChannelState,
    msg: IncomingMessage,
) -> Result<(), (StatusCode, Json<WebhookResponse>)> {
    let msg_id = msg.id;
    // Clone sender while holding read lock, then release lock before async send.
    // This prevents blocking other webhook handlers during the async I/O.
    let tx = {
        let guard = state.tx.read().await;
        guard.as_ref().cloned()
    };

    let Some(tx) = tx else {
        return Err(error_response(
            msg_id,
            StatusCode::SERVICE_UNAVAILABLE,
            "Channel not started",
        ));
    };
    if tx.send(msg).await.is_err() {
        return Err(error_response(
            msg_id,
            StatusCode::INTERNAL_SERVER_ERROR,
            "Channel closed",
        ));
    }
    Ok(())
}

/// Await the agent's response with a 60-second timeout, mapping timeout and
/// cancellation to explanatory text.
async fn await_agent_response(rx: oneshot::Receiver<String>) -> String {
    match tokio::time::timeout(std::time::Duration::from_secs(60), rx).await {
        Ok(Ok(content)) => content,
        Ok(Err(_)) => "Response cancelled".to_string(),
        Err(_) => "Response timeout".to_string(),
    }
}

async fn process_message(
    state: Arc<HttpChannelState>,
    msg: IncomingMessage,
    wait_for_response: bool,
) -> (StatusCode, Json<WebhookResponse>) {
    let msg_id = msg.id;

    // Set up response channel if waiting
    let response_rx = if wait_for_response {
        match register_response_channel(&state, msg_id).await {
            Ok(rx) => Some(rx),
            Err(err) => return err,
        }
    } else {
        None
    };

    if let Err(err) = forward_to_agent(&state, msg).await {
        return err;
    }

    // Wait for response if requested
    let response = match response_rx {
        Some(rx) => Some(await_agent_response(rx).await),
        None => None,
    };

    // Ensure pending response entry is cleaned up on timeout or cancellation
    let _ = state.pending_responses.write().await.remove(&msg_id);

    (
        StatusCode::OK,
        Json(WebhookResponse {
            message_id: msg_id,
            status: "accepted".to_string(),
            response,
        }),
    )
}
