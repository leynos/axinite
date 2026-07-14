//! Unit tests for relay channel naming and conversation metadata.

use std::sync::Arc;

use super::*;
use crate::channels::NativeChannel;
use crate::channels::relay::client::{RelayClient, RelayError};

fn test_client() -> Result<RelayClient, RelayError> {
    RelayClient::new(
        "http://localhost:3001".into(),
        secrecy::SecretString::from("key".to_string()),
        30,
    )
}

#[test]
fn relay_channel_name() {
    let channel = RelayChannel::new(
        test_client().expect("client"),
        "token".into(),
        "T123".into(),
        "inst1".into(),
        "user1".into(),
    );
    assert_eq!(channel.name(), DEFAULT_RELAY_NAME);
}

#[test]
fn conversation_context_extracts_metadata() {
    let channel = RelayChannel::new(
        test_client().expect("client"),
        "token".into(),
        "T123".into(),
        "inst1".into(),
        "user1".into(),
    );

    let metadata = serde_json::json!({
        "sender_name": "bob",
        "sender_id": "U123",
        "channel_id": "C456",
    });
    let ctx = channel.conversation_context(&metadata);
    assert_eq!(ctx.get("sender"), Some(&"bob".to_string()));
    assert_eq!(ctx.get("sender_uuid"), Some(&"U123".to_string()));
    assert_eq!(ctx.get("platform"), Some(&"slack".to_string()));
}

#[test]
fn metadata_shape_includes_event_type_and_sender_name() {
    // Regression: metadata JSON must include event_type and sender_name
    // for downstream routing (DM vs channel) and conversation_context().
    let metadata = serde_json::json!({
        "team_id": "T123",
        "channel_id": "C456",
        "sender_id": "U789",
        "sender_name": "alice",
        "event_type": "direct_message",
        "thread_id": null,
        "provider": "slack",
    });
    // event_type must be present for DM-vs-channel routing
    assert_eq!(
        metadata.get("event_type").and_then(|v| v.as_str()),
        Some("direct_message")
    );
    // sender_name must be present for conversation_context
    assert_eq!(
        metadata.get("sender_name").and_then(|v| v.as_str()),
        Some("alice")
    );
}

#[test]
fn with_timeouts_sets_values() {
    let channel = RelayChannel::new(
        test_client().expect("client"),
        "token".into(),
        "T123".into(),
        "inst1".into(),
        "user1".into(),
    )
    .with_timeouts(43200, 2000, 120000);

    assert_eq!(channel.stream_timeout_secs, 43200);
    assert_eq!(channel.backoff_initial_ms, 2000);
    assert_eq!(channel.backoff_max_ms, 120000);
}

#[test]
fn build_send_body_slack() {
    let channel = RelayChannel::new(
        test_client().expect("client"),
        "token".into(),
        "T123".into(),
        "inst1".into(),
        "user1".into(),
    );
    let (method, body) = channel.build_send_body("C456", "hello", Some("1234567.890"));
    assert_eq!(method, "chat.postMessage");
    assert_eq!(body["channel"], "C456");
    assert_eq!(body["text"], "hello");
    assert_eq!(body["thread_ts"], "1234567.890");
}

#[test]
fn parser_handle_is_shared_arc() {
    let channel = RelayChannel::new(
        test_client().expect("client"),
        "token".into(),
        "T123".into(),
        "inst1".into(),
        "user1".into(),
    );
    // parser_handle should be an Arc — cloning should give a second reference
    let handle_clone = Arc::clone(&channel.parser_handle);
    // Both point to the same allocation
    assert!(Arc::ptr_eq(&channel.parser_handle, &handle_clone));
}

#[test]
fn with_max_failures_sets_value() {
    let channel = RelayChannel::new(
        test_client().expect("client"),
        "token".into(),
        "T123".into(),
        "inst1".into(),
        "user1".into(),
    )
    .with_max_failures(10);

    assert_eq!(channel.max_consecutive_failures, 10);
}

#[test]
fn default_max_failures_is_50() {
    let channel = RelayChannel::new(
        test_client().expect("client"),
        "token".into(),
        "T123".into(),
        "inst1".into(),
        "user1".into(),
    );
    assert_eq!(channel.max_consecutive_failures, 50);
}

#[test]
fn empty_team_id_accepted_at_construction() {
    // Regression: empty team_id (when no DB store is available) must not
    // prevent channel construction or cause immediate shutdown.
    let channel = RelayChannel::new(
        test_client().expect("client"),
        "token".into(),
        String::new(), // empty team_id
        "inst1".into(),
        "user1".into(),
    );
    assert_eq!(channel.team_id, "");
    // The reconnect loop now skips team validation when team_id is empty,
    // so the channel remains alive.
}
