//! Image sentinel tests.
//!
//! Unit tests exercise JSON extraction logic inline. Delegate-level tests
//! exercise `ChatDelegate::maybe_emit_image_sentinel` through a real
//! `ChannelManager` with a `StubChannel`, verifying that SSE status events
//! are emitted or skipped correctly.

use std::sync::Arc;

use crate::channels::StatusUpdate;
use crate::context::JobContext;
use crate::testing::StubChannel;

use super::*;

// === Unit tests for inline JSON extraction logic ===

#[test]
fn test_image_sentinel_empty_data_url_should_be_skipped() {
    // Regression: unwrap_or_default() on missing "data" field produces an empty
    // string. Broadcasting an empty data_url would send a broken SSE event.
    let sentinel = serde_json::json!({
        "type": "image_generated",
        "path": "/tmp/image.png"
        // "data" field is missing
    });

    let data_url = sentinel
        .get("data")
        .and_then(|v| v.as_str())
        .unwrap_or_default()
        .to_string();

    assert!(
        data_url.is_empty(),
        "Missing 'data' field should produce empty string"
    );
    // The fix: empty data_url means we skip broadcasting
}

#[test]
fn test_image_sentinel_present_data_url_is_valid() {
    let sentinel = serde_json::json!({
        "type": "image_generated",
        "data": "data:image/png;base64,abc123",
        "path": "/tmp/image.png"
    });

    let data_url = sentinel
        .get("data")
        .and_then(|v| v.as_str())
        .unwrap_or_default()
        .to_string();

    assert!(
        !data_url.is_empty(),
        "Present 'data' field should produce non-empty string"
    );
}

#[test]
fn test_image_sentinel_http_url_is_invalid() {
    let sentinel = serde_json::json!({
        "type": "image_generated",
        "data": "https://example.test/image.png",
        "path": "/tmp/image.png"
    });

    let data_url = sentinel
        .get("data")
        .and_then(|v| v.as_str())
        .filter(|value| value.starts_with("data:image/"));

    assert!(
        data_url.is_none(),
        "Non-data URLs should be rejected for image sentinel payloads"
    );
}

// === Delegate-level tests ===

/// Build a minimal `Agent` wired to a `ChannelManager` with a registered stub.
fn build_agent_with_stub_channel(channels: Arc<ChannelManager>) -> Agent {
    Agent::new(
        make_agent_config(10, true),
        make_agent_deps(Arc::new(MockLlmProvider::static_ok()), false),
        channels,
        None,
        None,
        None,
        Some(Arc::new(ContextManager::new(1))),
        None,
    )
}

async fn new_stubbed_channels(
    name: &str,
) -> (
    Arc<ChannelManager>,
    Arc<std::sync::Mutex<Vec<StatusUpdate>>>,
) {
    let (stub, _sender) = StubChannel::new(name);
    let statuses = stub.captured_statuses_handle();
    let channels = Arc::new(ChannelManager::new());
    channels.add(Box::new(stub)).await;
    (channels, statuses)
}

fn make_delegate<'a>(
    agent: &'a Agent,
    session: Arc<Mutex<Session>>,
    message: &'a IncomingMessage,
) -> super::super::delegate::ChatDelegate<'a> {
    super::super::delegate::ChatDelegate {
        agent,
        session,
        thread_id: uuid::Uuid::new_v4(),
        message,
        job_ctx: JobContext::with_user(&message.user_id, &message.channel, "test session"),
        active_skills: vec![],
        cached_prompt: String::new(),
        cached_prompt_no_tools: String::new(),
        nudge_at: 0,
        force_text_at: 0,
        user_tz: chrono_tz::UTC,
    }
}

fn sentinel_json(data: Option<&str>, path: Option<&str>) -> String {
    let mut sentinel = serde_json::Map::from_iter([(
        "type".to_string(),
        serde_json::Value::String("image_generated".to_string()),
    )]);
    if let Some(data) = data {
        sentinel.insert(
            "data".to_string(),
            serde_json::Value::String(data.to_string()),
        );
    }
    if let Some(path) = path {
        sentinel.insert(
            "path".to_string(),
            serde_json::Value::String(path.to_string()),
        );
    }
    serde_json::Value::Object(sentinel).to_string()
}

#[tokio::test]
async fn delegate_emits_image_generated_for_valid_data_url() {
    let (channels, statuses) = new_stubbed_channels("test-chan").await;
    let agent = build_agent_with_stub_channel(channels);
    let session = Arc::new(Mutex::new(Session::new("test-user")));
    let message = IncomingMessage::new("test-chan", "test-user", "generate an image");

    let delegate = make_delegate(&agent, session, &message);

    let output = sentinel_json(Some("data:image/png;base64,abc123"), Some("/tmp/image.png"));

    let result = delegate
        .maybe_emit_image_sentinel("image_generate", &output)
        .await;

    assert!(
        result,
        "should return true for image_generate with valid sentinel"
    );

    let captured = statuses.lock().expect("statuses lock poisoned");
    assert_eq!(captured.len(), 1, "should have emitted exactly one status");
    match &captured[0] {
        StatusUpdate::ImageGenerated { data_url, path } => {
            assert_eq!(data_url, "data:image/png;base64,abc123");
            assert_eq!(path.as_deref(), Some("/tmp/image.png"));
        }
        other => panic!("Expected ImageGenerated, got {:?}", other),
    }
}

#[tokio::test]
async fn delegate_skips_broadcast_when_data_url_is_empty() {
    let (channels, statuses) = new_stubbed_channels("test-chan").await;
    let agent = build_agent_with_stub_channel(channels);
    let session = Arc::new(Mutex::new(Session::new("test-user")));
    let message = IncomingMessage::new("test-chan", "test-user", "generate an image");

    let delegate = make_delegate(&agent, session, &message);

    // Missing "data" field — empty data URL
    let output = sentinel_json(None, Some("/tmp/image.png"));

    let result = delegate
        .maybe_emit_image_sentinel("image_generate", &output)
        .await;

    assert!(
        result,
        "should return true (sentinel detected) even when data is empty"
    );

    let captured = statuses.lock().expect("statuses lock poisoned");
    assert!(
        captured.is_empty(),
        "should NOT emit any status when data URL is empty"
    );
}

#[tokio::test]
async fn delegate_skips_broadcast_when_data_url_is_not_a_data_url() {
    let (channels, statuses) = new_stubbed_channels("test-chan").await;
    let agent = build_agent_with_stub_channel(channels);
    let session = Arc::new(Mutex::new(Session::new("test-user")));
    let message = IncomingMessage::new("test-chan", "test-user", "generate an image");

    let delegate = make_delegate(&agent, session, &message);

    let output = sentinel_json(
        Some("https://example.test/image.png"),
        Some("/tmp/image.png"),
    );

    let result = delegate
        .maybe_emit_image_sentinel("image_generate", &output)
        .await;

    assert!(
        result,
        "should return true (sentinel detected) even when data URL is invalid"
    );

    let captured = statuses.lock().expect("statuses lock poisoned");
    assert!(
        captured.is_empty(),
        "should NOT emit any status when data URL is not a data URL"
    );
}

#[tokio::test]
async fn delegate_returns_false_for_non_image_tool() {
    let (channels, statuses) = new_stubbed_channels("test-chan").await;
    let agent = build_agent_with_stub_channel(channels);
    let session = Arc::new(Mutex::new(Session::new("test-user")));
    let message = IncomingMessage::new("test-chan", "test-user", "do something");

    let delegate = make_delegate(&agent, session, &message);

    let output = sentinel_json(Some("data:image/png;base64,abc123"), None);

    let result = delegate.maybe_emit_image_sentinel("echo", &output).await;

    assert!(!result, "should return false for non-image tool");

    let captured = statuses.lock().expect("statuses lock poisoned");
    assert!(
        captured.is_empty(),
        "should NOT emit any status for non-image tool"
    );
}
