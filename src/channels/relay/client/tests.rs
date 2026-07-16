//! Unit tests for relay event deserialization and client helpers.

use tokio::sync::mpsc;

use super::*;

#[test]
fn channel_event_deserialize_minimal() {
    let json = r#"{"event_type": "message", "content": "hello"}"#;
    let event: ChannelEvent = serde_json::from_str(json).expect("parse failed");
    assert_eq!(event.event_type, "message");
    assert_eq!(event.text(), "hello");
    assert!(event.provider_scope.is_empty());
}

#[test]
fn channel_event_deserialize_relay_format() {
    // Matches the actual channel-relay ChannelEvent serialization format.
    let json = r#"{
        "id": "evt_123",
        "event_type": "direct_message",
        "provider": "slack",
        "provider_scope": "T123",
        "channel_id": "D456",
        "sender_id": "U789",
        "sender_name": "bob",
        "content": "hi there",
        "thread_id": "1234567890.123456",
        "raw": {},
        "timestamp": "2026-03-09T21:00:00Z"
    }"#;
    let event: ChannelEvent = serde_json::from_str(json).expect("parse failed");
    assert_eq!(event.provider, "slack");
    assert_eq!(event.team_id(), "T123");
    assert_eq!(event.display_name(), "bob");
    assert_eq!(event.thread_id, Some("1234567890.123456".to_string()));
    assert!(event.is_message());
}

#[test]
fn channel_event_is_message() {
    let make = |et: &str| ChannelEvent {
        id: String::new(),
        event_type: et.to_string(),
        provider: String::new(),
        provider_scope: String::new(),
        channel_id: String::new(),
        sender_id: String::new(),
        sender_name: None,
        content: None,
        thread_id: None,
        raw: serde_json::Value::Null,
        timestamp: None,
    };
    assert!(make("message").is_message());
    assert!(make("direct_message").is_message());
    assert!(make("mention").is_message());
    assert!(!make("reaction").is_message());
}

#[test]
fn connection_deserialize() {
    let json =
        r#"{"provider": "slack", "team_id": "T123", "team_name": "My Team", "connected": true}"#;
    let conn: Connection = serde_json::from_str(json).expect("parse failed");
    assert_eq!(conn.provider, "slack");
    assert!(conn.connected);
}

#[test]
fn relay_error_display() {
    let err = RelayError::Network("timeout".into());
    assert_eq!(err.to_string(), "Network error: timeout");

    let err = RelayError::Api {
        status: 401,
        message: "unauthorized".into(),
    };
    assert_eq!(err.to_string(), "API error (HTTP 401): unauthorized");

    let err = RelayError::TokenExpired;
    assert_eq!(err.to_string(), "Stream token expired");
}

#[test]
fn event_type_constants_match_is_message() {
    let make = |et: &str| ChannelEvent {
        id: String::new(),
        event_type: et.to_string(),
        provider: String::new(),
        provider_scope: String::new(),
        channel_id: String::new(),
        sender_id: String::new(),
        sender_name: None,
        content: None,
        thread_id: None,
        raw: serde_json::Value::Null,
        timestamp: None,
    };
    assert!(make(event_types::MESSAGE).is_message());
    assert!(make(event_types::DIRECT_MESSAGE).is_message());
    assert!(make(event_types::MENTION).is_message());
}

#[tokio::test]
async fn parse_sse_handles_multibyte_utf8_across_chunks() {
    // The crab emoji (🦀) is 4 bytes: [0xF0, 0x9F, 0xA6, 0x80].
    // Split it across two chunks to verify no U+FFFD corruption.
    let event_json = r#"{"event_type":"message","content":"hello 🦀 world","provider_scope":"T1","channel_id":"C1","sender_id":"U1"}"#;
    let full = format!("event: message\ndata: {}\n\n", event_json);
    let bytes = full.as_bytes();

    // Find the crab emoji and split mid-character
    let crab_pos = bytes
        .windows(4)
        .position(|w| w == [0xF0, 0x9F, 0xA6, 0x80])
        .expect("crab emoji not found");
    let split_at = crab_pos + 2; // split in the middle of the 4-byte emoji

    let chunk1 = bytes::Bytes::copy_from_slice(&bytes[..split_at]);
    let chunk2 = bytes::Bytes::copy_from_slice(&bytes[split_at..]);

    let chunks: Vec<Result<bytes::Bytes, reqwest::Error>> = vec![Ok(chunk1), Ok(chunk2)];
    let stream = futures::stream::iter(chunks);

    let (tx, mut rx) = mpsc::channel(8);
    parse_sse_stream(stream, tx).await;

    let event = rx.recv().await.expect("should receive event");
    assert_eq!(event.text(), "hello 🦀 world");
}
