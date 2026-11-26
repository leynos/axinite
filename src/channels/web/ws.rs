//! WebSocket handler for bidirectional client communication.
//!
//! Provides the same event stream as SSE but also accepts incoming messages
//! (chat, approvals) over a single persistent connection.
//!
//! ```text
//! Client ──── WS frame: {"type":"message","content":"hello"} ──► Agent Loop
//!        ◄─── WS frame: {"type":"event","event_type":"response","data":{...}} ── Broadcast
//!        ──── WS frame: {"type":"ping"} ──────────────────────────────────────►
//!        ◄─── WS frame: {"type":"pong"} ──────────────────────────────────────
//! ```

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use axum::extract::ws::{Message, WebSocket};
use futures::{SinkExt, StreamExt};
use tokio::sync::mpsc;
use uuid::Uuid;

use crate::agent::submission::Submission;
use crate::channels::IncomingMessage;
use crate::channels::web::server::GatewayState;
use crate::channels::web::types::{SseEvent, WsClientMessage, WsServerMessage};
use chat_message::ChatMessage;

/// Tracks active WebSocket connections.
pub struct WsConnectionTracker {
    count: AtomicU64,
}

impl WsConnectionTracker {
    /// Create a tracker with no open connections.
    pub fn new() -> Self {
        Self {
            count: AtomicU64::new(0),
        }
    }

    /// Number of WebSocket connections currently open.
    pub fn connection_count(&self) -> u64 {
        self.count.load(Ordering::Relaxed)
    }

    fn increment(&self) {
        self.count.fetch_add(1, Ordering::Relaxed);
    }

    fn decrement(&self) {
        self.count.fetch_sub(1, Ordering::Relaxed);
    }
}

impl Default for WsConnectionTracker {
    fn default() -> Self {
        Self::new()
    }
}

/// Handle an upgraded WebSocket connection.
///
/// Spawns two tasks:
/// - **sender**: forwards broadcast events to the WebSocket client
/// - **receiver**: reads client frames and routes them to the agent
///
/// When either task ends (client disconnect or broadcast closed), both are
/// cleaned up.
pub async fn handle_ws_connection(socket: WebSocket, state: Arc<GatewayState>) {
    let (ws_sink, mut ws_stream) = socket.split();

    // Track connection
    let tracker_for_drop = state.ws_tracker.clone();
    adjust_tracker(tracker_for_drop.as_deref(), WsTrackerChange::Opened);

    // Subscribe to broadcast events (same source as SSE).
    // Reject if we've hit the connection limit.
    let Some(raw_stream) = state.sse.subscribe_raw() else {
        tracing::warn!("WebSocket rejected: too many connections");
        // Decrement the WS tracker we already incremented above.
        adjust_tracker(tracker_for_drop.as_deref(), WsTrackerChange::Closed);
        return;
    };
    let event_stream = Box::pin(raw_stream);

    // Channel for the sender task to receive messages from both
    // the broadcast stream and any direct sends (like Pong)
    let (direct_tx, direct_rx) = mpsc::channel::<WsServerMessage>(64);

    // Sender task: forward broadcast events + direct messages to WS client
    let sender_handle = tokio::spawn(forward_outbound(ws_sink, event_stream, direct_rx));

    // Receiver task: read client frames and route to agent
    receive_frames(&mut ws_stream, &state, &direct_tx).await;

    // Clean up: abort sender, decrement counter
    sender_handle.abort();
    adjust_tracker(tracker_for_drop.as_deref(), WsTrackerChange::Closed);
}

/// Forward broadcast events and direct messages to the client until it disconnects.
async fn forward_outbound<S>(
    mut ws_sink: futures::stream::SplitSink<WebSocket, Message>,
    mut event_stream: S,
    mut direct_rx: mpsc::Receiver<WsServerMessage>,
) where
    S: futures::Stream<Item = SseEvent> + Unpin,
{
    while let Some(msg) = next_outbound(&mut event_stream, &mut direct_rx).await {
        let json = match serde_json::to_string(&msg) {
            Ok(j) => j,
            Err(_) => continue,
        };

        if ws_sink.send(Message::Text(json.into())).await.is_err() {
            break; // Client disconnected
        }
    }
}

/// Read client frames and route them to the agent until the client closes or errors.
async fn receive_frames(
    ws_stream: &mut futures::stream::SplitStream<WebSocket>,
    state: &GatewayState,
    direct_tx: &mpsc::Sender<WsServerMessage>,
) {
    while let Some(Ok(frame)) = ws_stream.next().await {
        if handle_frame(frame, state, &state.user_id, direct_tx)
            .await
            .is_break()
        {
            break;
        }
    }
}

/// A change to the open-connection count.
enum WsTrackerChange {
    Opened,
    Closed,
}

/// Apply `change` to the connection tracker, when one is configured.
fn adjust_tracker(tracker: Option<&WsConnectionTracker>, change: WsTrackerChange) {
    let Some(tracker) = tracker else {
        return;
    };
    match change {
        WsTrackerChange::Opened => tracker.increment(),
        WsTrackerChange::Closed => tracker.decrement(),
    }
}

/// Wait for the next message to send to the client: a broadcast event or a
/// direct message. `None` means either source closed.
async fn next_outbound<S>(
    event_stream: &mut S,
    direct_rx: &mut mpsc::Receiver<WsServerMessage>,
) -> Option<WsServerMessage>
where
    S: futures::Stream<Item = SseEvent> + Unpin,
{
    tokio::select! {
        event = event_stream.next() => {
            event.map(|sse_event| WsServerMessage::from_sse_event(&sse_event))
        }
        direct = direct_rx.recv() => direct,
    }
}

/// Route one client frame; `Break` means the client closed the connection.
async fn handle_frame(
    frame: Message,
    state: &GatewayState,
    user_id: &str,
    direct_tx: &mpsc::Sender<WsServerMessage>,
) -> std::ops::ControlFlow<()> {
    match frame {
        Message::Text(text) => {
            match serde_json::from_str::<WsClientMessage>(&text) {
                Ok(client_msg) => {
                    handle_client_message(client_msg, state, user_id, direct_tx).await;
                }
                Err(e) => {
                    let _ = direct_tx
                        .send(WsServerMessage::Error {
                            message: format!("Invalid message: {}", e),
                        })
                        .await;
                }
            }
            std::ops::ControlFlow::Continue(())
        }
        Message::Close(_) => std::ops::ControlFlow::Break(()),
        // Ignore binary, ping/pong (axum handles protocol-level pings)
        _ => std::ops::ControlFlow::Continue(()),
    }
}

/// Route a parsed client message to the appropriate handler.
async fn handle_client_message(
    msg: WsClientMessage,
    state: &GatewayState,
    user_id: &str,
    direct_tx: &mpsc::Sender<WsServerMessage>,
) {
    let ctx = WsContext {
        state,
        user_id,
        direct_tx,
    };
    match msg {
        WsClientMessage::Message {
            content,
            thread_id,
            timezone,
            images,
        } => {
            let chat = ChatMessage {
                content,
                thread_id,
                timezone,
                images,
            };
            forward_chat_message(chat, &ctx).await;
        }
        WsClientMessage::Approval {
            request_id,
            action,
            thread_id,
        } => {
            forward_approval(&request_id, &action, thread_id.as_deref(), &ctx).await;
        }
        WsClientMessage::AuthToken {
            extension_name,
            token,
        } => {
            submit_auth_token(extension_name, &token, &ctx).await;
        }
        WsClientMessage::AuthCancel { extension_name } => {
            crate::channels::web::server::clear_auth_mode(state, Some(&extension_name)).await;
        }
        WsClientMessage::Ping => {
            let _ = direct_tx.send(WsServerMessage::Pong).await;
        }
    }
}

/// What a client message handler needs: the gateway, the user and the reply channel.
struct WsContext<'a> {
    state: &'a GatewayState,
    user_id: &'a str,
    direct_tx: &'a mpsc::Sender<WsServerMessage>,
}

impl WsContext<'_> {
    /// Send an error frame straight back to the client, ignoring a closed channel.
    async fn send_error(&self, message: impl Into<String>) {
        let _ = self
            .direct_tx
            .send(WsServerMessage::Error {
                message: message.into(),
            })
            .await;
    }
}

/// Forward a chat message to the agent, or tell the client why that is not possible.
async fn forward_chat_message(chat: ChatMessage, ctx: &WsContext<'_>) {
    let incoming = chat.into_incoming(ctx.user_id);

    let tx_guard = ctx.state.msg_tx.read().await;
    if let Some(ref tx) = *tx_guard {
        if tx.send(incoming).await.is_err() {
            ctx.send_error("Channel closed").await;
        }
    } else {
        ctx.send_error("Channel not started").await;
    }
}

/// Map an approval action to `(approved, always)`.
fn parse_approval_action(action: &str) -> Option<(bool, bool)> {
    match action {
        "approve" => Some((true, false)),
        "always" => Some((true, true)),
        "deny" => Some((false, false)),
        _ => None,
    }
}

/// Turn an approval reply into an `ExecApproval` submission and queue it for the agent.
async fn forward_approval(
    request_id: &str,
    action: &str,
    thread_id: Option<&str>,
    ctx: &WsContext<'_>,
) {
    let Some((approved, always)) = parse_approval_action(action) else {
        ctx.send_error(format!("Unknown approval action: {}", action))
            .await;
        return;
    };

    let Ok(request_uuid) = Uuid::parse_str(request_id) else {
        ctx.send_error("Invalid request_id (expected UUID)").await;
        return;
    };

    let approval = Submission::ExecApproval {
        request_id: request_uuid,
        approved,
        always,
    };
    let content = match serde_json::to_string(&approval) {
        Ok(c) => c,
        Err(e) => {
            ctx.send_error(format!("Failed to serialize approval: {}", e))
                .await;
            return;
        }
    };

    let mut msg = IncomingMessage::new("gateway", ctx.user_id, content);
    if let Some(tid) = thread_id {
        msg = msg.with_thread(tid);
    }
    let tx_guard = ctx.state.msg_tx.read().await;
    if let Some(ref tx) = *tx_guard {
        let _ = tx.send(msg).await;
    }
}

/// Authenticate an extension with a client-supplied token and broadcast the outcome.
async fn submit_auth_token(extension_name: String, token: &str, ctx: &WsContext<'_>) {
    let Some(ref ext_mgr) = ctx.state.extension_manager else {
        ctx.send_error("Extension manager not available").await;
        return;
    };

    match ext_mgr.auth(&extension_name, Some(token)).await {
        Ok(result) if result.is_authenticated() => {
            let message = match ext_mgr.activate(&extension_name).await {
                Ok(r) => format!(
                    "{} authenticated ({} tools loaded)",
                    extension_name,
                    r.tools_loaded.len()
                ),
                Err(e) => format!(
                    "{} authenticated but activation failed: {}",
                    extension_name, e
                ),
            };
            crate::channels::web::server::clear_auth_mode(ctx.state, Some(&extension_name)).await;
            ctx.state
                .sse
                .broadcast(crate::channels::web::types::SseEvent::AuthCompleted {
                    extension_name,
                    success: true,
                    message,
                });
        }
        Ok(result) => {
            ctx.state
                .sse
                .broadcast(crate::channels::web::types::SseEvent::AuthRequired {
                    extension_name,
                    instructions: result.instructions().map(String::from),
                    auth_url: result.auth_url().map(String::from),
                    setup_url: result.setup_url().map(String::from),
                });
        }
        Err(e) => {
            ctx.send_error(format!("Auth failed: {}", e)).await;
        }
    }
}

mod chat_message;

#[cfg(test)]
mod tests;
