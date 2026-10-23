//! Tests for deserialization of signal-cli SSE envelope JSON shapes.

use super::*;

#[test]
fn sse_envelope_deserializes() {
    let json = r#"{
        "envelope": {
            "source": "+1111111111",
            "sourceNumber": "+1111111111",
            "sourceName": "Test User",
            "timestamp": 1700000000000,
            "dataMessage": {
                "message": "Hello Signal!",
                "timestamp": 1700000000000
            }
        }
    }"#;
    let sse: SseEnvelope = serde_json::from_str(json).expect("fixture JSON should deserialize");
    let env = sse
        .envelope
        .expect("SSE payload should contain an envelope");
    assert_eq!(env.source_number.as_deref(), Some("+1111111111"));
    assert_eq!(env.source_name.as_deref(), Some("Test User"));
    let dm = env
        .data_message
        .expect("envelope should contain a data message");
    assert_eq!(dm.message.as_deref(), Some("Hello Signal!"));
}

#[test]
fn sse_envelope_deserializes_group() {
    let json = r#"{
        "envelope": {
            "sourceNumber": "+2222222222",
            "dataMessage": {
                "message": "Group msg",
                "groupInfo": {
                    "groupId": "abc123"
                }
            }
        }
    }"#;
    let sse: SseEnvelope = serde_json::from_str(json).expect("fixture JSON should deserialize");
    let env = sse
        .envelope
        .expect("SSE payload should contain an envelope");
    let dm = env
        .data_message
        .expect("envelope should contain a data message");
    assert_eq!(
        dm.group_info
            .as_ref()
            .expect("data message should contain group info")
            .group_id
            .as_deref(),
        Some("abc123")
    );
}

#[test]
fn envelope_defaults() {
    let json = r#"{}"#;
    let env: Envelope = serde_json::from_str(json).expect("fixture JSON should deserialize");
    assert!(env.source.is_none());
    assert!(env.source_number.is_none());
    assert!(env.source_name.is_none());
    assert!(env.data_message.is_none());
    assert!(env.story_message.is_none());
    assert!(env.timestamp.is_none());
}

// ── SSE envelope deserialization edge cases ─────────────────────

#[test]
fn sse_envelope_missing_envelope_field() {
    let json = r#"{"account": "+1234567890"}"#;
    let sse: SseEnvelope = serde_json::from_str(json).expect("fixture JSON should deserialize");
    assert!(sse.envelope.is_none());
}

#[test]
fn sse_envelope_with_story_message() {
    let json = r#"{
        "envelope": {
            "sourceNumber": "+1111111111",
            "storyMessage": {"allowsReplies": true},
            "dataMessage": {
                "message": "story text"
            }
        }
    }"#;
    let sse: SseEnvelope = serde_json::from_str(json).expect("fixture JSON should deserialize");
    let env = sse
        .envelope
        .expect("SSE payload should contain an envelope");
    assert!(env.story_message.is_some());
    assert!(env.data_message.is_some());
}

#[test]
fn sse_envelope_with_attachments() {
    let json = r#"{
        "envelope": {
            "sourceNumber": "+1111111111",
            "dataMessage": {
                "message": "See attached",
                "attachments": [
                    {"contentType": "image/jpeg", "filename": "photo.jpg"},
                    {"contentType": "application/pdf"}
                ]
            }
        }
    }"#;
    let sse: SseEnvelope = serde_json::from_str(json).expect("fixture JSON should deserialize");
    let dm = sse
        .envelope
        .expect("SSE payload should contain an envelope")
        .data_message
        .expect("envelope should contain a data message");
    let attachments = dm
        .attachments
        .expect("data message should contain attachments");
    assert_eq!(attachments.len(), 2);
}
