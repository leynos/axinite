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

    let mut parser = SseParser::default();

    let mut byte_stream = std::pin::pin!(byte_stream);
    while let Some(chunk_result) = byte_stream.next().await {
        let chunk = match chunk_result {
            Ok(c) => c,
            Err(e) => {
                tracing::debug!(error = %e, "SSE stream chunk error");
                break;
            }
        };

        parser.push(&chunk);

        while let Some(line) = parser.take_line() {
            if let Some(event) = parser.accept_line(&line)
                && tx.send(event).await.is_err()
            {
                return; // receiver dropped
            }
        }
    }

    tracing::debug!("SSE stream ended");
}

/// Incremental SSE parser: buffers bytes, splits lines and assembles events.
#[derive(Default)]
struct SseParser {
    buffer: Vec<u8>,
    event_type: String,
    data_lines: Vec<String>,
}

impl SseParser {
    /// Append a chunk of bytes to the line buffer.
    fn push(&mut self, chunk: &[u8]) {
        self.buffer.extend_from_slice(chunk);
    }

    /// Remove and return the next complete line, if any.
    ///
    /// UTF-8 is decoded only on full lines to avoid corruption when multi-byte
    /// characters span chunk boundaries.
    fn take_line(&mut self) -> Option<String> {
        let newline_pos = self.buffer.iter().position(|&b| b == b'\n')?;
        let line = String::from_utf8_lossy(&self.buffer[..newline_pos])
            .trim_end_matches('\r')
            .to_string();
        self.buffer.drain(..=newline_pos);
        Some(line)
    }

    /// Feed one line; a blank line ends the event and yields it when it parses.
    fn accept_line(&mut self, line: &str) -> Option<ChannelEvent> {
        if line.is_empty() {
            return self.finish_event();
        }
        if let Some(value) = line.strip_prefix("event:") {
            self.event_type = value.trim().to_string();
        } else if let Some(value) = line.strip_prefix("data:") {
            self.data_lines.push(value.trim().to_string());
        }
        // Ignore other fields (id:, retry:, comments)
        None
    }

    /// End the current event: parse its data and reset the accumulators.
    fn finish_event(&mut self) -> Option<ChannelEvent> {
        let event = self.parse_event();
        self.event_type.clear();
        self.data_lines.clear();
        event
    }

    /// Parse the accumulated data lines as a `ChannelEvent`, defaulting its
    /// type to the `event:` field.
    fn parse_event(&self) -> Option<ChannelEvent> {
        if self.data_lines.is_empty() {
            return None;
        }
        let data = self.data_lines.join("\n");
        match serde_json::from_str::<ChannelEvent>(&data) {
            Ok(mut event) => {
                if event.event_type.is_empty() && !self.event_type.is_empty() {
                    event.event_type = self.event_type.clone();
                }
                Some(event)
            }
            Err(_) => {
                tracing::debug!(
                    event_type = %self.event_type,
                    data_len = data.len(),
                    "Failed to parse SSE event data as ChannelEvent"
                );
                None
            }
        }
    }
}

#[cfg(test)]
mod parser_tests {
    //! Unit tests for the incremental SSE line parser.

    use super::*;

    fn feed(parser: &mut SseParser, input: &str) -> Vec<ChannelEvent> {
        parser.push(input.as_bytes());
        let mut events = Vec::new();
        while let Some(line) = parser.take_line() {
            events.extend(parser.accept_line(&line));
        }
        events
    }

    #[test]
    fn blank_line_ends_an_event_and_resets_the_accumulators() {
        let mut parser = SseParser::default();
        let events = feed(
            &mut parser,
            "event: message\r\ndata: {\"event_type\":\"\"}\n\n",
        );
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].event_type, "message");
        assert!(parser.event_type.is_empty() && parser.data_lines.is_empty());
    }

    #[test]
    fn an_event_type_in_the_payload_wins_over_the_event_field() {
        let mut parser = SseParser::default();
        let events = feed(
            &mut parser,
            "event: other\ndata: {\"event_type\":\"mention\"}\n\n",
        );
        assert_eq!(events[0].event_type, "mention");
    }

    #[test]
    fn lines_split_across_chunks_are_reassembled() {
        let mut parser = SseParser::default();
        assert!(feed(&mut parser, "data: {\"event_ty").is_empty());
        let events = feed(&mut parser, "pe\":\"message\"}\n\n");
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].event_type, "message");
    }

    #[test]
    fn unparseable_data_and_unknown_fields_yield_no_event() {
        let mut parser = SseParser::default();
        let events = feed(&mut parser, ": comment\nid: 7\ndata: not json\n\n");
        assert!(events.is_empty());
        assert!(parser.data_lines.is_empty());
    }
}
