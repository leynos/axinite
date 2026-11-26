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
use super::attachments::{AttachmentData, decode_attachments};
use crate::channels::{IncomingAttachment, IncomingMessage};

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

/// A webhook reply: status code plus JSON body.
type WebhookReply = (StatusCode, Json<WebhookResponse>);

pub(super) async fn webhook_handler(
    State(state): State<Arc<HttpChannelState>>,
    Json(req): Json<WebhookRequest>,
) -> WebhookReply {
    match validate_webhook(&state, &req).await {
        Ok(msg) => process_message(state, msg, req.wait_for_response).await,
        Err(reply) => reply,
    }
}

/// Run every webhook admission check in order and build the message to forward.
async fn validate_webhook(
    state: &HttpChannelState,
    req: &WebhookRequest,
) -> Result<IncomingMessage, WebhookReply> {
    check_rate_limit(state).await?;

    let _ = req.user_id.as_ref().map(|user_id| {
        tracing::debug!(
            provided_user_id = %user_id,
            "HTTP webhook request provided user_id, ignoring in favour of configured user_id"
        );
    });

    check_webhook_secret(state, req.secret.as_deref()).await?;

    if req.content.len() > MAX_CONTENT_BYTES {
        return Err(error_response(
            Uuid::nil(),
            StatusCode::PAYLOAD_TOO_LARGE,
            "Content too large",
        ));
    }

    let attachments = decode_attachments(&req.attachments)
        .map_err(|rejection| error_response(Uuid::nil(), rejection.status, &rejection.message))?;
    Ok(build_incoming_message(state, req, attachments))
}

/// Count this request against the fixed one-minute window; fail with 429 when
/// the window's budget is spent.
async fn check_rate_limit(state: &HttpChannelState) -> Result<(), WebhookReply> {
    let mut limiter = state.rate_limit.lock().await;
    if limiter.window_start.elapsed() >= std::time::Duration::from_secs(60) {
        limiter.window_start = std::time::Instant::now();
        limiter.request_count = 0;
    }
    limiter.request_count += 1;
    if limiter.request_count > MAX_REQUESTS_PER_MINUTE {
        return Err(error_response(
            Uuid::nil(),
            StatusCode::TOO_MANY_REQUESTS,
            "Rate limit exceeded",
        ));
    }
    Ok(())
}

/// Compare the provided secret against the configured one in constant time.
///
/// Passes when no secret is configured; fails with 401 when one is configured
/// and the request omits it or supplies a different value.
async fn check_webhook_secret(
    state: &HttpChannelState,
    provided: Option<&str>,
) -> Result<(), WebhookReply> {
    let guard = state.webhook_secret.read().await;
    let Some(expected_secret) = guard.as_ref() else {
        return Ok(());
    };
    let expected_bytes = expected_secret.expose_secret().as_bytes();
    match provided {
        Some(p) if bool::from(p.as_bytes().ct_eq(expected_bytes)) => Ok(()),
        Some(_) => Err(error_response(
            Uuid::nil(),
            StatusCode::UNAUTHORIZED,
            "Invalid webhook secret",
        )),
        None => Err(error_response(
            Uuid::nil(),
            StatusCode::UNAUTHORIZED,
            "Webhook secret required",
        )),
    }
}

/// Assemble the message forwarded to the agent from a validated request.
fn build_incoming_message(
    state: &HttpChannelState,
    req: &WebhookRequest,
    attachments: Vec<IncomingAttachment>,
) -> IncomingMessage {
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
    msg
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
