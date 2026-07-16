//! SSE stream parsing for the channel-relay event feed.
//!
//! Provides `ChannelEventStream`, an async stream of parsed channel events,
//! and the background task that decodes the SSE wire format into events.

use std::pin::Pin;
use std::task::{Context, Poll};

use futures::Stream;
use tokio::sync::mpsc;

use super::events::ChannelEvent;

/// Async stream of parsed channel events from SSE.
pub struct ChannelEventStream {
    pub(super) rx: mpsc::Receiver<ChannelEvent>,
}

impl Stream for ChannelEventStream {
    type Item = ChannelEvent;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        self.rx.poll_recv(cx)
    }
}

/// Parse SSE format from a reqwest bytes stream.
///
/// SSE format:
/// ```text
/// event: message
/// data: {"key": "value"}
///
/// ```
/// Blank line terminates an event.
pub(super) async fn parse_sse_stream(
    byte_stream: impl futures::Stream<Item = Result<bytes::Bytes, reqwest::Error>> + Send + 'static,
    tx: mpsc::Sender<ChannelEvent>,
) {
    use futures::StreamExt;

    let mut buffer = Vec::<u8>::new();
    let mut event_type = String::new();
    let mut data_lines = Vec::new();

    let mut byte_stream = std::pin::pin!(byte_stream);
    while let Some(chunk_result) = byte_stream.next().await {
        let chunk = match chunk_result {
            Ok(c) => c,
            Err(e) => {
                tracing::debug!(error = %e, "SSE stream chunk error");
                break;
            }
        };

        buffer.extend_from_slice(&chunk);

        // Process complete lines (decode UTF-8 only on full lines to avoid
        // corruption when multi-byte characters span chunk boundaries)
        while let Some(newline_pos) = buffer.iter().position(|&b| b == b'\n') {
            let line = String::from_utf8_lossy(&buffer[..newline_pos])
                .trim_end_matches('\r')
                .to_string();
            buffer.drain(..=newline_pos);

            if line.is_empty() {
                // Blank line = end of event
                if !data_lines.is_empty() {
                    let data = data_lines.join("\n");
                    if let Ok(mut event) = serde_json::from_str::<ChannelEvent>(&data) {
                        if event.event_type.is_empty() && !event_type.is_empty() {
                            event.event_type = event_type.clone();
                        }
                        if tx.send(event).await.is_err() {
                            return; // receiver dropped
                        }
                    } else {
                        tracing::debug!(
                            event_type = %event_type,
                            data_len = data.len(),
                            "Failed to parse SSE event data as ChannelEvent"
                        );
                    }
                }
                event_type.clear();
                data_lines.clear();
            } else if let Some(value) = line.strip_prefix("event:") {
                event_type = value.trim().to_string();
            } else if let Some(value) = line.strip_prefix("data:") {
                data_lines.push(value.trim().to_string());
            }
            // Ignore other fields (id:, retry:, comments)
        }
    }

    tracing::debug!("SSE stream ended");
}
