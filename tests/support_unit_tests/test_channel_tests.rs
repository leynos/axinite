//! Test channel helper tests.

use std::sync::Arc;
use std::time::Duration;

use crate::support::test_channel::TestChannel;
use ironclaw::channels::{Channel, IncomingMessage, OutgoingResponse, StatusUpdate};

#[tokio::test]
async fn send_and_receive_message() {
    let channel = TestChannel::new();
    let mut stream = channel.start().await.unwrap();

    channel.send_message("hello world").await;

    use futures::StreamExt;
    let msg = stream.next().await.expect("stream should yield a message");
    assert_eq!(msg.content, "hello world");
    assert_eq!(msg.channel, "test");
    assert_eq!(msg.user_id, "test-user");
}

#[tokio::test]
async fn captures_responses() {
    let channel = TestChannel::new();
    let incoming = IncomingMessage::new("test", "test-user", "hi");

    channel
        .respond(&incoming, OutgoingResponse::text("reply 1"))
        .await
        .unwrap();
    channel
        .respond(&incoming, OutgoingResponse::text("reply 2"))
        .await
        .unwrap();

    let captured = channel.captured_responses();
    assert_eq!(captured.len(), 2);
    assert_eq!(captured[0].content, "reply 1");
    assert_eq!(captured[1].content, "reply 2");
}

#[tokio::test]
async fn captures_status_events() {
    let channel = TestChannel::new();
    let metadata = serde_json::Value::Null;

    channel
        .send_status(
            StatusUpdate::ToolStarted {
                name: "echo".to_string(),
            },
            &metadata,
        )
        .await
        .unwrap();
    channel
        .send_status(
            StatusUpdate::ToolCompleted {
                name: "echo".to_string(),
                success: true,
                error: None,
                parameters: None,
            },
            &metadata,
        )
        .await
        .unwrap();

    let events = channel.captured_status_events();
    assert_eq!(events.len(), 2);
    assert!(matches!(&events[0], StatusUpdate::ToolStarted { name } if name == "echo"));
    assert!(
        matches!(&events[1], StatusUpdate::ToolCompleted { name, success, .. } if name == "echo" && *success)
    );
}

#[tokio::test]
async fn tool_calls_started() {
    let channel = TestChannel::new();
    let metadata = serde_json::Value::Null;

    channel
        .send_status(
            StatusUpdate::ToolStarted {
                name: "memory_search".to_string(),
            },
            &metadata,
        )
        .await
        .unwrap();
    channel
        .send_status(StatusUpdate::Thinking("hmm".to_string()), &metadata)
        .await
        .unwrap();
    channel
        .send_status(
            StatusUpdate::ToolStarted {
                name: "echo".to_string(),
            },
            &metadata,
        )
        .await
        .unwrap();

    let started = channel.tool_calls_started();
    assert_eq!(started, vec!["memory_search", "echo"]);
}

#[tokio::test]
async fn tool_results() {
    let channel = TestChannel::new();
    channel
        .send_status(
            StatusUpdate::ToolResult {
                name: "echo".to_string(),
                preview: "hello world".to_string(),
            },
            &serde_json::Value::Null,
        )
        .await
        .unwrap();
    channel
        .send_status(
            StatusUpdate::ToolResult {
                name: "time".to_string(),
                preview: "{\"iso\": \"2026-03-03\"}".to_string(),
            },
            &serde_json::Value::Null,
        )
        .await
        .unwrap();

    let results = channel.tool_results();
    assert_eq!(results.len(), 2);
    assert_eq!(results[0].0, "echo");
    assert_eq!(results[0].1, "hello world");
    assert_eq!(results[1].0, "time");
    assert!(results[1].1.contains("2026"));
}

#[tokio::test]
async fn wait_for_responses() {
    let channel = TestChannel::new();
    let responses = Arc::clone(&channel.responses);

    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(100)).await;
        responses
            .lock()
            .await
            .push(OutgoingResponse::text("delayed reply"));
    });

    let collected = channel.wait_for_responses(1, Duration::from_secs(2)).await;
    assert_eq!(collected.len(), 1);
    assert_eq!(collected[0].content, "delayed reply");
}

#[tokio::test]
async fn tool_timings() {
    let channel = TestChannel::new();
    channel
        .send_status(
            StatusUpdate::ToolStarted {
                name: "echo".to_string(),
            },
            &serde_json::Value::Null,
        )
        .await
        .unwrap();
    tokio::time::sleep(Duration::from_millis(50)).await;
    channel
        .send_status(
            StatusUpdate::ToolCompleted {
                name: "echo".to_string(),
                success: true,
                error: None,
                parameters: None,
            },
            &serde_json::Value::Null,
        )
        .await
        .unwrap();

    let timings = channel.tool_timings();
    assert_eq!(timings.len(), 1);
    assert_eq!(timings[0].0, "echo");
    assert!(
        timings[0].1 >= 40,
        "Expected >= 40ms, got {}ms",
        timings[0].1
    );
}
