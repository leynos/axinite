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

    let mut url = reqwest::Url::parse(&format!("{}/api/v1/events", channel.config.http_url))
        .map_err(|e| ChannelError::StartupFailed {
            name: "signal".to_string(),
            reason: format!("Invalid SSE URL: {e}"),
        })?;
    url.query_pairs_mut()
        .append_pair("account", &channel.config.account);

    let mut retry_delay = Duration::from_secs(2);
    let max_delay = Duration::from_secs(60);

    loop {
        let resp = channel
            .client
            .get(url.clone())
            .header("Accept", "text/event-stream")
            .send()
            .await;

        let resp = match resp {
            Ok(r) if r.status().is_success() => r,
            Ok(r) => {
                let status = r.status();
                let mut stream = r.bytes_stream();
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
                let body = String::from_utf8_lossy(&bytes);
                tracing::warn!("Signal SSE returned {status}: {body}");
                tokio::time::sleep(retry_delay).await;
                retry_delay = (retry_delay * 2).min(max_delay);
                continue;
            }
            Err(e) => {
                let safe_url = SignalChannel::redact_url(url.as_str());
                tracing::warn!("Signal SSE connect error to {safe_url}: {e}, retrying...");
                tokio::time::sleep(retry_delay).await;
                retry_delay = (retry_delay * 2).min(max_delay);
                continue;
            }
        };

        // Connection succeeded — reset backoff.
        retry_delay = Duration::from_secs(2);
        tracing::info!("Signal SSE connected");

        let mut bytes_stream = resp.bytes_stream();
        let mut buffer = String::with_capacity(8192);
        let mut current_data = String::with_capacity(4096);
        // Holds trailing bytes from the previous chunk that form an incomplete
        // multi-byte UTF-8 sequence. At most 3 bytes (the longest incomplete
        // leading sequence for a 4-byte character).
        let mut utf8_carry: Vec<u8> = Vec::with_capacity(4);

        while let Some(chunk) = bytes_stream.next().await {
            let chunk = match chunk {
                Ok(c) => c,
                Err(e) => {
                    tracing::debug!("Signal SSE chunk error, reconnecting: {e}");
                    break;
                }
            };

            // Prepend any leftover bytes from the previous chunk.
            let decode_buf = if utf8_carry.is_empty() {
                chunk.to_vec()
            } else {
                let mut combined = std::mem::take(&mut utf8_carry);
                combined.extend_from_slice(&chunk);
                combined
            };

            // Decode as much valid UTF-8 as possible, carrying over any
            // incomplete trailing sequence to the next iteration.
            let (valid_len, carry_start) = match std::str::from_utf8(&decode_buf) {
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
                        None => {
                            // Incomplete multi-byte sequence at the end – carry it over.
                            (valid_up_to, valid_up_to)
                        }
                    }
                }
            };

            use std::borrow::Cow;

            debug_assert!(
                std::str::from_utf8(&decode_buf[..valid_len]).is_ok(),
                "valid_len {} should be a valid UTF-8 boundary (buffer len: {})",
                valid_len,
                decode_buf.len()
            );

            let text: Cow<str> = match std::str::from_utf8(&decode_buf[..valid_len]) {
                Ok(s) => Cow::Borrowed(s),
                Err(_) => {
                    tracing::warn!(
                        "Signal SSE: unexpected invalid UTF-8 boundary at valid_len {}, \
                         falling back to lossy conversion",
                        valid_len
                    );
                    Cow::Owned(String::from_utf8_lossy(&decode_buf[..valid_len]).into_owned())
                }
            };

            if buffer.len() + text.len() > MAX_SSE_BUFFER_SIZE {
                tracing::warn!(
                    "Signal SSE buffer overflow, resetting: buffer_len={} text_len={} max={}",
                    buffer.len(),
                    text.len(),
                    MAX_SSE_BUFFER_SIZE
                );
                buffer.clear();
                utf8_carry.clear();
                current_data.clear();
                continue;
            }
            buffer.push_str(&text);

            // Preserve any trailing incomplete bytes for the next chunk.
            if carry_start < decode_buf.len() {
                utf8_carry.extend_from_slice(&decode_buf[carry_start..]);
            }

            while let Some(newline_pos) = buffer.find('\n') {
                let line = buffer[..newline_pos].trim_end_matches('\r').to_string();
                buffer.drain(..=newline_pos);

                // Skip SSE comments (keepalive).
                if line.starts_with(':') {
                    continue;
                }

                if line.is_empty() {
                    // Empty line = event boundary, dispatch accumulated data.
                    if !current_data.is_empty() {
                        match serde_json::from_str::<SseEnvelope>(&current_data) {
                            Ok(sse) => {
                                if let Some(ref envelope) = sse.envelope
                                    && let Some((msg, target)) = channel.process_envelope(envelope)
                                {
                                    // Handle /debug command locally (same as REPL).
                                    let content_lower = msg.content.trim().to_lowercase();
                                    if content_lower == "/debug" {
                                        let new_state = channel.toggle_debug();
                                        let response = if new_state {
                                            "Debug mode enabled. Tool execution will be shown in chat."
                                        } else {
                                            "Debug mode disabled. Tool execution will be hidden from chat."
                                        };
                                        let reply_params = channel.build_rpc_params(
                                            &SignalChannel::parse_recipient_target(&target),
                                            Some(response),
                                            None,
                                        );
                                        let _ = channel.rpc_request("send", reply_params).await;
                                        // Don't send the /debug command to the agent.
                                        continue;
                                    }

                                    // Store reply target for respond().
                                    // LruCache automatically evicts the
                                    // least-recently-used entry when full.
                                    {
                                        let mut targets = reply_targets.write().await;
                                        targets.put(msg.id, target);
                                    }
                                    if tx.send(msg).await.is_err() {
                                        tracing::debug!("Signal SSE: receiver dropped, exiting");
                                        return Ok(());
                                    }
                                }
                            }
                            Err(e) => {
                                tracing::debug!("Signal SSE parse skip: {e}");
                            }
                        }
                        current_data.clear();
                    }
                } else if let Some(data) = line.strip_prefix("data:") {
                    if current_data.len() + data.len() > MAX_SSE_EVENT_SIZE {
                        tracing::warn!("Signal SSE event too large, dropping");
                        current_data.clear();
                        continue;
                    }
                    if !current_data.is_empty() {
                        current_data.push('\n');
                    }
                    current_data.push_str(data.trim_start());
                }
                // Ignore "event:", "id:", "retry:" lines.
            }
        }

        // Process any trailing data before reconnect.
        let trailing = if current_data.is_empty() {
            None
        } else {
            serde_json::from_str::<SseEnvelope>(&current_data).ok()
        };
        let trailing_envelope = trailing.as_ref().and_then(|sse| sse.envelope.as_ref());
        if let Some(envelope) = trailing_envelope
            && let Some((msg, target)) = channel.process_envelope(envelope)
        {
            reply_targets.write().await.put(msg.id, target);
            let _ = tx.send(msg).await;
        }

        tracing::debug!("Signal SSE stream ended, reconnecting with backoff...");
        tokio::time::sleep(retry_delay).await;
        retry_delay = std::cmp::min(retry_delay * 2, max_delay);
    }
}
