//! Typing-task lifecycle tests for the WASM channel wrapper: spawn on
//! `Thinking`, cancellation on terminal statuses, persistence across tool
//! starts, replacement on repeated `Thinking`, and cancellation on respond.

use super::*;

/// Runs the canonical typing-task lifecycle test:
///
/// 1. Start the channel.
/// 2. Send `Thinking` and assert the typing task is spawned.
/// 3. Send `second_status` and assert the typing task is either
///    cancelled (`expect_cancelled = true`) or still live (`false`).
/// 4. Shut down cleanly.
async fn assert_typing_task_after_status(
    second_status: crate::channels::StatusUpdate,
    expect_cancelled: bool,
) {
    let channel = create_test_channel();
    let _stream = channel.start().await.expect("Channel should start");

    let metadata = serde_json::json!({"chat_id": 123});

    // Establish a typing task
    let _ = channel
        .send_status(
            crate::channels::StatusUpdate::Thinking("Processing...".into()),
            &metadata,
        )
        .await;
    assert!(channel.typing_task.read().await.is_some());

    // Apply the second status under test
    let _ = channel.send_status(second_status, &metadata).await;

    if expect_cancelled {
        assert!(
            channel.typing_task.read().await.is_none(),
            "expected typing task to be cancelled"
        );
    } else {
        assert!(
            channel.typing_task.read().await.is_some(),
            "expected typing task to persist"
        );
    }

    channel.shutdown().await.expect("Shutdown should succeed");
}

#[tokio::test]
async fn test_typing_task_starts_on_thinking() {
    let channel = create_test_channel();
    let _stream = channel.start().await.expect("Channel should start");

    let metadata = serde_json::json!({"chat_id": 123});

    // Sending Thinking should succeed (no-op for no WASM)
    let result = channel
        .send_status(
            crate::channels::StatusUpdate::Thinking("Processing...".into()),
            &metadata,
        )
        .await;
    assert!(result.is_ok());

    // A typing task should have been spawned
    assert!(channel.typing_task.read().await.is_some());

    // Shutdown should cancel the typing task
    channel.shutdown().await.expect("Shutdown should succeed");
    assert!(channel.typing_task.read().await.is_none());
}

#[tokio::test]
async fn test_typing_task_cancelled_on_done() {
    assert_typing_task_after_status(crate::channels::StatusUpdate::Status("Done".into()), true)
        .await;
}

#[tokio::test]
async fn test_typing_task_persists_on_tool_started() {
    assert_typing_task_after_status(
        crate::channels::StatusUpdate::ToolStarted {
            name: "http_request".into(),
        },
        false,
    )
    .await;
}

#[tokio::test]
async fn test_typing_task_cancelled_on_approval_needed() {
    assert_typing_task_after_status(
        crate::channels::StatusUpdate::ApprovalNeeded {
            request_id: "req-1".into(),
            tool_name: "http_request".into(),
            description: "Fetch weather".into(),
            parameters: serde_json::json!({"url": "https://wttr.in"}),
        },
        true,
    )
    .await;
}

#[tokio::test]
async fn test_typing_task_cancelled_on_awaiting_approval_status() {
    assert_typing_task_after_status(
        crate::channels::StatusUpdate::Status("Awaiting approval".into()),
        true,
    )
    .await;
}

#[tokio::test]
async fn test_typing_task_replaced_on_new_thinking() {
    let channel = create_test_channel();
    let _stream = channel.start().await.expect("Channel should start");

    let metadata = serde_json::json!({"chat_id": 123});

    // Start typing
    let _ = channel
        .send_status(
            crate::channels::StatusUpdate::Thinking("First...".into()),
            &metadata,
        )
        .await;

    // Get handle of first task
    let first_handle = {
        let guard = channel.typing_task.read().await;
        guard.as_ref().map(|h| h.id())
    };
    assert!(first_handle.is_some());

    // Start typing again (should replace the previous task)
    let _ = channel
        .send_status(
            crate::channels::StatusUpdate::Thinking("Second...".into()),
            &metadata,
        )
        .await;

    // Should still have a typing task, but it's a new one
    let second_handle = {
        let guard = channel.typing_task.read().await;
        guard.as_ref().map(|h| h.id())
    };
    assert!(second_handle.is_some());
    // The task IDs should differ (old one was aborted, new one spawned)
    assert_ne!(first_handle, second_handle);

    channel.shutdown().await.expect("Shutdown should succeed");
}

#[tokio::test]
async fn test_respond_cancels_typing_task() {
    use crate::channels::IncomingMessage;

    let channel = create_test_channel();
    let _stream = channel.start().await.expect("Channel should start");

    let metadata = serde_json::json!({"chat_id": 123});

    // Start typing
    let _ = channel
        .send_status(
            crate::channels::StatusUpdate::Thinking("Processing...".into()),
            &metadata,
        )
        .await;
    assert!(channel.typing_task.read().await.is_some());

    // Respond should cancel the typing task
    let msg = IncomingMessage::new("test", "user1", "hello").with_metadata(metadata);
    let _ = channel
        .respond(&msg, crate::channels::OutgoingResponse::text("response"))
        .await;

    // Typing task should be gone
    assert!(channel.typing_task.read().await.is_none());

    channel.shutdown().await.expect("Shutdown should succeed");
}

#[tokio::test]
async fn test_stream_chunk_is_noop() {
    let channel = create_test_channel();
    let _stream = channel.start().await.expect("Channel should start");

    let metadata = serde_json::json!({"chat_id": 123});

    // StreamChunk should not start a typing task
    let result = channel
        .send_status(
            crate::channels::StatusUpdate::StreamChunk("chunk".into()),
            &metadata,
        )
        .await;
    assert!(result.is_ok());
    assert!(channel.typing_task.read().await.is_none());

    channel.shutdown().await.expect("Shutdown should succeed");
}
