//! Tests for message emission, per-execution limits, and rate limiting.

use crate::channels::wasm::capabilities::{ChannelCapabilities, EmitRateLimitConfig};
use crate::channels::wasm::host::{
    ChannelEmitRateLimiter, ChannelHostState, EmittedMessage, MAX_EMITS_PER_EXECUTION,
};

#[test]
fn test_emit_message_basic() {
    let caps = ChannelCapabilities::for_channel("test");
    let mut state = ChannelHostState::new("test", caps);

    let msg = EmittedMessage::new("user123", "Hello, world!");
    state.emit_message(msg).unwrap();

    assert_eq!(state.emitted_count(), 1);

    let messages = state.take_emitted_messages();
    assert_eq!(messages.len(), 1);
    assert_eq!(messages[0].user_id, "user123");
    assert_eq!(messages[0].content, "Hello, world!");

    // Queue should be cleared
    assert_eq!(state.emitted_count(), 0);
}

#[test]
fn test_emit_message_with_metadata() {
    let caps = ChannelCapabilities::for_channel("test");
    let mut state = ChannelHostState::new("test", caps);

    let msg = EmittedMessage::new("user123", "Hello")
        .with_user_name("John Doe")
        .with_thread_id("thread-1")
        .with_metadata(r#"{"key": "value"}"#);

    state.emit_message(msg).unwrap();

    let messages = state.take_emitted_messages();
    assert_eq!(messages[0].user_name, Some("John Doe".to_string()));
    assert_eq!(messages[0].thread_id, Some("thread-1".to_string()));
    assert_eq!(messages[0].metadata_json, r#"{"key": "value"}"#);
}

#[test]
fn test_emit_per_execution_limit() {
    let caps = ChannelCapabilities::for_channel("test");
    let mut state = ChannelHostState::new("test", caps);

    // Fill up to limit
    for i in 0..MAX_EMITS_PER_EXECUTION {
        let msg = EmittedMessage::new("user", format!("Message {}", i));
        state.emit_message(msg).unwrap();
    }

    // This should be dropped silently
    let msg = EmittedMessage::new("user", "Should be dropped");
    state.emit_message(msg).unwrap();

    assert_eq!(state.emitted_count(), MAX_EMITS_PER_EXECUTION);
    assert_eq!(state.emits_dropped(), 1);
}

#[test]
fn test_rate_limiter_basic() {
    let config = EmitRateLimitConfig {
        messages_per_minute: 10,
        messages_per_hour: 100,
    };
    let mut limiter = ChannelEmitRateLimiter::new(config);

    // Should allow 10 messages
    for _ in 0..10 {
        assert!(limiter.check_and_record());
    }

    // 11th should be blocked
    assert!(!limiter.check_and_record());
}

#[test]
fn test_channel_name() {
    let caps = ChannelCapabilities::for_channel("telegram");
    let state = ChannelHostState::new("telegram", caps);

    assert_eq!(state.channel_name(), "telegram");
}

#[test]
fn test_emit_and_take_preserves_order_and_content() {
    // Emit multiple messages, take them, verify order and content.
    let caps = ChannelCapabilities::for_channel("discord");
    let mut state = ChannelHostState::new("discord", caps);

    let messages_data = vec![
        ("user-a", "Hello from A"),
        ("user-b", "Hello from B"),
        ("user-a", "Follow-up from A"),
    ];
    for (uid, content) in &messages_data {
        state
            .emit_message(EmittedMessage::new(*uid, *content))
            .unwrap();
    }

    assert_eq!(state.emitted_count(), 3);

    let taken = state.take_emitted_messages();
    assert_eq!(taken.len(), 3);

    // Order preserved.
    for (i, (uid, content)) in messages_data.iter().enumerate() {
        assert_eq!(taken[i].user_id, *uid);
        assert_eq!(taken[i].content, *content);
    }

    // Take empties the queue.
    assert_eq!(state.emitted_count(), 0);
    let taken2 = state.take_emitted_messages();
    assert!(taken2.is_empty());
}
