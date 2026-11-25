//! Long-running SSE listener for the Signal channel: connects to the
//! signal-cli events stream, decodes UTF-8 chunk boundaries, parses SSE
//! frames, and forwards accepted messages with exponential-backoff reconnect.

use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::time::Duration;

use futures::StreamExt;
use lru::LruCache;
use reqwest::Client;
use tokio::sync::RwLock;
use uuid::Uuid;

use crate::channels::IncomingMessage;
use crate::config::SignalConfig;
use crate::error::ChannelError;

use super::{
    MAX_ERROR_LOG_BODY, MAX_SSE_BUFFER_SIZE, MAX_SSE_EVENT_SIZE, SignalChannel, SseEnvelope,
};

const INITIAL_RETRY_DELAY: Duration = Duration::from_secs(2);
const MAX_RETRY_DELAY: Duration = Duration::from_secs(60);

/// Where accepted messages go: the channel, the reply-target cache and the agent queue.
struct SseSink<'a> {
    channel: &'a SignalChannel,
    reply_targets: &'a RwLock<LruCache<Uuid, String>>,
    tx: &'a tokio::sync::mpsc::Sender<IncomingMessage>,
}

/// What dispatching one SSE event asks of the line loop.
enum EventOutcome {
    /// The event was handled or skipped; clear the accumulated data.
    Done,
    /// The `/debug` command was handled locally; keep the accumulated data.
    DebugHandled,
    /// The receiver was dropped; the listener must exit.
    ReceiverDropped,
}

/// Accumulated SSE decoding state for one connection.
pub(super) struct SseState {
    pub(super) buffer: String,
    pub(super) current_data: String,
    /// Trailing bytes from the previous chunk that form an incomplete multi-byte UTF-8
    /// sequence: at most 3 (the longest incomplete leading sequence of a 4-byte character).
    pub(super) utf8_carry: Vec<u8>,
}

impl SseState {
    pub(super) fn new() -> Self {
        Self {
            buffer: String::with_capacity(8192),
            current_data: String::with_capacity(4096),
            utf8_carry: Vec::with_capacity(4),
        }
    }

    /// Decode a chunk into the line buffer; return `false` when the buffer overflowed and was reset.
    pub(super) fn ingest(&mut self, chunk: &[u8]) -> bool {
        // Prepend any leftover bytes from the previous chunk.
        let decode_buf = if self.utf8_carry.is_empty() {
            chunk.to_vec()
        } else {
            let mut combined = std::mem::take(&mut self.utf8_carry);
            combined.extend_from_slice(chunk);
            combined
        };

        let (valid_len, carry_start) = utf8_split_points(&decode_buf);
        let text = utf8_text(&decode_buf, valid_len);

        if self.buffer.len() + text.len() > MAX_SSE_BUFFER_SIZE {
            tracing::warn!(
                "Signal SSE buffer overflow, resetting: buffer_len={} text_len={} max={}",
                self.buffer.len(),
                text.len(),
                MAX_SSE_BUFFER_SIZE
            );
            self.buffer.clear();
            self.utf8_carry.clear();
            self.current_data.clear();
            return false;
        }
        self.buffer.push_str(&text);

        // Preserve any trailing incomplete bytes for the next chunk.
        if carry_start < decode_buf.len() {
            self.utf8_carry
                .extend_from_slice(&decode_buf[carry_start..]);
        }
        true
    }

    /// Accumulate one `data:` line, dropping the event when it grows too large.
    pub(super) fn push_data_line(&mut self, data: &str) {
        if self.current_data.len() + data.len() > MAX_SSE_EVENT_SIZE {
            tracing::warn!("Signal SSE event too large, dropping");
            self.current_data.clear();
            return;
        }
        if !self.current_data.is_empty() {
            self.current_data.push('\n');
        }
        self.current_data.push_str(data.trim_start());
    }
}

/// Find where the valid UTF-8 prefix ends and where the carried-over tail starts.
pub(super) fn utf8_split_points(decode_buf: &[u8]) -> (usize, usize) {
    match std::str::from_utf8(decode_buf) {
        Ok(_) => (decode_buf.len(), decode_buf.len()),
        Err(e) => {
            let valid_up_to = e.valid_up_to();
            match e.error_len() {
                Some(bad_len) => {
                    // Genuinely invalid byte sequence (not just incomplete).
                    // Skip the bad byte(s) and keep going with what we have.
                    tracing::debug!(
                        "Signal SSE invalid UTF-8 byte at offset {valid_up_to}, \
                         skipping"
                    );
                    // Advance past the bad byte(s); remaining data (if any)
                    // will be carried over to the next chunk.
                    (valid_up_to, valid_up_to + bad_len)
                }
                // Incomplete multi-byte sequence at the end – carry it over.
                None => (valid_up_to, valid_up_to),
            }
        }
    }
}

/// View the first `valid_len` bytes as text, falling back to lossy conversion on a bad boundary.
fn utf8_text(decode_buf: &[u8], valid_len: usize) -> std::borrow::Cow<'_, str> {
    use std::borrow::Cow;

    debug_assert!(
        std::str::from_utf8(&decode_buf[..valid_len]).is_ok(),
        "valid_len {} should be a valid UTF-8 boundary (buffer len: {})",
        valid_len,
        decode_buf.len()
    );

    match std::str::from_utf8(&decode_buf[..valid_len]) {
        Ok(s) => Cow::Borrowed(s),
        Err(_) => {
            tracing::warn!(
                "Signal SSE: unexpected invalid UTF-8 boundary at valid_len {}, \
                 falling back to lossy conversion",
                valid_len
            );
            Cow::Owned(String::from_utf8_lossy(&decode_buf[..valid_len]).into_owned())
        }
    }
}

/// Build the events URL, with the account as a query parameter.
fn events_url(channel: &SignalChannel) -> Result<reqwest::Url, ChannelError> {
    let mut url = reqwest::Url::parse(&format!("{}/api/v1/events", channel.config.http_url))
        .map_err(|e| ChannelError::StartupFailed {
            name: "signal".to_string(),
            reason: format!("Invalid SSE URL: {e}"),
        })?;
    url.query_pairs_mut()
        .append_pair("account", &channel.config.account);
    Ok(url)
}

/// Sleep for the current delay, then double it up to the maximum.
async fn back_off(retry_delay: &mut Duration) {
    tokio::time::sleep(*retry_delay).await;
    *retry_delay = (*retry_delay * 2).min(MAX_RETRY_DELAY);
}

/// Read at most `MAX_ERROR_LOG_BODY` bytes of an error response body for logging.
async fn read_error_body(resp: reqwest::Response) -> String {
    let mut stream = resp.bytes_stream();
    let mut bytes = Vec::new();
    let mut collected = 0usize;
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.unwrap_or_default();
        let remaining = MAX_ERROR_LOG_BODY.saturating_sub(collected);
        if remaining == 0 {
            break;
        }
        bytes.extend_from_slice(&chunk[..chunk.len().min(remaining)]);
        collected = bytes.len();
        if collected >= MAX_ERROR_LOG_BODY {
            break;
        }
    }
    String::from_utf8_lossy(&bytes).into_owned()
}

/// Open the SSE stream; on failure log, back off and return `None` so the caller retries.
async fn connect(
    channel: &SignalChannel,
    url: &reqwest::Url,
    retry_delay: &mut Duration,
) -> Option<reqwest::Response> {
    let resp = channel
        .client
        .get(url.clone())
        .header("Accept", "text/event-stream")
        .send()
        .await;

    match resp {
        Ok(r) if r.status().is_success() => Some(r),
        Ok(r) => {
            let status = r.status();
            let body = read_error_body(r).await;
            tracing::warn!("Signal SSE returned {status}: {body}");
            back_off(retry_delay).await;
            None
        }
        Err(e) => {
            let safe_url = SignalChannel::redact_url(url.as_str());
            tracing::warn!("Signal SSE connect error to {safe_url}: {e}, retrying...");
            back_off(retry_delay).await;
            None
        }
    }
}

/// Handle the `/debug` command locally (same as REPL): toggle debug mode and reply.
async fn handle_debug_command(channel: &SignalChannel, target: &str) {
    let new_state = channel.toggle_debug();
    let response = if new_state {
        "Debug mode enabled. Tool execution will be shown in chat."
    } else {
        "Debug mode disabled. Tool execution will be hidden from chat."
    };
    let reply_params = channel.build_rpc_params(
        &SignalChannel::parse_recipient_target(target),
        Some(response),
        None,
    );
    let _ = channel.rpc_request("send", reply_params).await;
}

/// Parse one complete SSE event and forward its message to the agent.
async fn dispatch_event(sink: &SseSink<'_>, current_data: &str) -> EventOutcome {
    let sse = match serde_json::from_str::<SseEnvelope>(current_data) {
        Ok(sse) => sse,
        Err(e) => {
            tracing::debug!("Signal SSE parse skip: {e}");
            return EventOutcome::Done;
        }
    };
    let Some(ref envelope) = sse.envelope else {
        return EventOutcome::Done;
    };
    let Some((msg, target)) = sink.channel.process_envelope(envelope) else {
        return EventOutcome::Done;
    };

    if msg.content.trim().to_lowercase() == "/debug" {
        handle_debug_command(sink.channel, &target).await;
        // Don't send the /debug command to the agent.
        return EventOutcome::DebugHandled;
    }

    // Store reply target for respond().
    // LruCache automatically evicts the least-recently-used entry when full.
    sink.reply_targets.write().await.put(msg.id, target);
    if sink.tx.send(msg).await.is_err() {
        tracing::debug!("Signal SSE: receiver dropped, exiting");
        return EventOutcome::ReceiverDropped;
    }
    EventOutcome::Done
}

/// Consume complete lines from the buffer; return `true` when the receiver was dropped.
async fn drain_lines(state: &mut SseState, sink: &SseSink<'_>) -> bool {
    while let Some(newline_pos) = state.buffer.find('\n') {
        let line = state.buffer[..newline_pos]
            .trim_end_matches('\r')
            .to_string();
        state.buffer.drain(..=newline_pos);

        // Skip SSE comments (keepalive).
        if line.starts_with(':') {
            continue;
        }

        if line.is_empty() {
            // Empty line = event boundary, dispatch accumulated data.
            if state.current_data.is_empty() {
                continue;
            }
            match dispatch_event(sink, &state.current_data).await {
                EventOutcome::Done => state.current_data.clear(),
                EventOutcome::DebugHandled => {}
                EventOutcome::ReceiverDropped => return true,
            }
        } else if let Some(data) = line.strip_prefix("data:") {
            state.push_data_line(data);
        }
        // Ignore "event:", "id:", "retry:" lines.
    }
    false
}

/// Forward any trailing event left in the state when the stream ends.
async fn flush_trailing(state: &SseState, sink: &SseSink<'_>) {
    let trailing = if state.current_data.is_empty() {
        None
    } else {
        serde_json::from_str::<SseEnvelope>(&state.current_data).ok()
    };
    let trailing_envelope = trailing.as_ref().and_then(|sse| sse.envelope.as_ref());
    if let Some(envelope) = trailing_envelope
        && let Some((msg, target)) = sink.channel.process_envelope(envelope)
    {
        sink.reply_targets.write().await.put(msg.id, target);
        let _ = sink.tx.send(msg).await;
    }
}

/// Read one connection to its end; return `true` when the receiver was dropped.
async fn consume_stream(resp: reqwest::Response, sink: &SseSink<'_>) -> bool {
    let mut bytes_stream = resp.bytes_stream();
    let mut state = SseState::new();

    while let Some(chunk) = bytes_stream.next().await {
        let chunk = match chunk {
            Ok(c) => c,
            Err(e) => {
                tracing::debug!("Signal SSE chunk error, reconnecting: {e}");
                break;
            }
        };

        if !state.ingest(&chunk) {
            continue;
        }
        if drain_lines(&mut state, sink).await {
            return true;
        }
    }

    // Process any trailing data before reconnect.
    flush_trailing(&state, sink).await;
    false
}

/// Long-running SSE listener that reconnects with exponential backoff.
pub(super) async fn sse_listener(
    config: SignalConfig,
    client: Client,
    tx: tokio::sync::mpsc::Sender<IncomingMessage>,
    reply_targets: Arc<RwLock<LruCache<Uuid, String>>>,
    debug_mode: Arc<AtomicBool>,
) -> Result<(), ChannelError> {
    let channel = SignalChannel::from_parts(
        config,
        client,
        Arc::clone(&reply_targets),
        Arc::clone(&debug_mode),
    );
    let url = events_url(&channel)?;
    let sink = SseSink {
        channel: &channel,
        reply_targets: &reply_targets,
        tx: &tx,
    };

    let mut retry_delay = INITIAL_RETRY_DELAY;

    loop {
        let Some(resp) = connect(&channel, &url, &mut retry_delay).await else {
            continue;
        };

        // Connection succeeded — reset backoff.
        retry_delay = INITIAL_RETRY_DELAY;
        tracing::info!("Signal SSE connected");

        if consume_stream(resp, &sink).await {
            return Ok(());
        }

        tracing::debug!("Signal SSE stream ended, reconnecting with backoff...");
        back_off(&mut retry_delay).await;
    }
}
