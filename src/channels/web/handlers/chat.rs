//! Chat send, SSE, and WebSocket handlers for the web gateway.

use std::sync::Arc;

use axum::{
    Json, Router,
    extract::{State, WebSocketUpgrade},
    http::{HeaderMap, StatusCode, Uri},
    response::IntoResponse,
    routing::{get, post},
};

use crate::channels::IncomingMessage;
use crate::channels::web::handlers::{chat_auth, chat_history};
use crate::channels::web::server::GatewayState;
use crate::channels::web::types::{ImageData, SendMessageRequest, SendMessageResponse};

pub fn routes() -> Router<Arc<GatewayState>> {
    Router::new()
        .route("/api/chat/send", post(chat_send_handler))
        .route("/api/chat/events", get(chat_events_handler))
        .route("/api/chat/ws", get(chat_ws_handler))
        .merge(chat_auth::routes())
        .merge(chat_history::routes())
}

/// Convert web gateway `ImageData` to `IncomingAttachment` objects.
pub(crate) fn images_to_attachments(
    images: &[ImageData],
) -> Vec<crate::channels::IncomingAttachment> {
    use base64::Engine;

    images
        .iter()
        .enumerate()
        .filter_map(|(i, img)| {
            if !img.media_type.starts_with("image/") {
                tracing::warn!(
                    "Skipping image {i}: invalid media type '{}' (must start with 'image/')",
                    img.media_type
                );
                return None;
            }
            let data = match base64::engine::general_purpose::STANDARD.decode(&img.data) {
                Ok(d) => d,
                Err(e) => {
                    tracing::warn!("Skipping image {i}: invalid base64 data: {e}");
                    return None;
                }
            };
            Some(crate::channels::IncomingAttachment {
                id: format!("web-image-{i}"),
                kind: crate::channels::AttachmentKind::Image,
                mime_type: img.media_type.clone(),
                filename: Some(format!("image-{i}.{}", mime_to_ext(&img.media_type))),
                size_bytes: Some(data.len() as u64),
                source_url: None,
                storage_key: None,
                extracted_text: None,
                data,
                duration_secs: None,
            })
        })
        .collect()
}

fn mime_to_ext(mime: &str) -> &str {
    match mime {
        "image/png" => "png",
        "image/jpeg" => "jpg",
        "image/gif" => "gif",
        "image/webp" => "webp",
        "image/svg+xml" => "svg",
        _ => "jpg",
    }
}

pub async fn chat_send_handler(
    State(state): State<Arc<GatewayState>>,
    headers: HeaderMap,
    Json(req): Json<SendMessageRequest>,
) -> Result<(StatusCode, Json<SendMessageResponse>), (StatusCode, String)> {
    tracing::trace!(
        "[chat_send_handler] Received message: content_len={}, thread_id={:?}",
        req.content.len(),
        req.thread_id
    );

    if !state.chat_rate_limiter.check() {
        return Err((
            StatusCode::TOO_MANY_REQUESTS,
            "Rate limit exceeded. Try again shortly.".to_string(),
        ));
    }

    let mut msg = IncomingMessage::new("gateway", &state.user_id, &req.content);
    let tz = req
        .timezone
        .as_deref()
        .or_else(|| headers.get("X-Timezone").and_then(|v| v.to_str().ok()));
    if let Some(tz) = tz {
        msg = msg.with_timezone(tz);
    }

    if let Some(ref thread_id) = req.thread_id {
        msg = msg.with_thread(thread_id);
        msg = msg.with_metadata(serde_json::json!({"thread_id": thread_id}));
    }

    if !req.images.is_empty() {
        msg = msg.with_attachments(images_to_attachments(&req.images));
    }

    let msg_id = msg.id;

    let tx_guard = state.msg_tx.read().await;
    let tx = tx_guard.as_ref().ok_or((
        StatusCode::SERVICE_UNAVAILABLE,
        "Channel not started".to_string(),
    ))?;

    tx.send(msg).await.map_err(|_| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            "Channel closed".to_string(),
        )
    })?;

    Ok((
        StatusCode::ACCEPTED,
        Json(SendMessageResponse {
            message_id: msg_id,
            status: "accepted",
        }),
    ))
}

pub async fn chat_events_handler(
    State(state): State<Arc<GatewayState>>,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    let sse = state.sse.subscribe().ok_or((
        StatusCode::SERVICE_UNAVAILABLE,
        "Too many connections".to_string(),
    ))?;
    Ok((
        [("X-Accel-Buffering", "no"), ("Cache-Control", "no-cache")],
        sse,
    ))
}

pub async fn chat_ws_handler(
    headers: HeaderMap,
    ws: WebSocketUpgrade,
    State(state): State<Arc<GatewayState>>,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    let origin = headers
        .get("origin")
        .and_then(|v| v.to_str().ok())
        .ok_or_else(|| {
            (
                StatusCode::FORBIDDEN,
                "WebSocket Origin header required".to_string(),
            )
        })?;

    let host = origin
        .parse::<Uri>()
        .ok()
        .and_then(|uri| {
            uri.authority()
                .map(|authority| authority.host().to_string())
        })
        .unwrap_or_default();

    let is_local = matches!(host.as_str(), "localhost" | "127.0.0.1" | "[::1]");
    if !is_local {
        return Err((
            StatusCode::FORBIDDEN,
            "WebSocket origin not allowed".to_string(),
        ));
    }
    Ok(ws.on_upgrade(move |socket| crate::channels::web::ws::handle_ws_connection(socket, state)))
}
