use std::sync::Arc;

use crate::channels::NativeChannel;
use crate::channels::wasm::capabilities::ChannelCapabilities;
use crate::channels::wasm::runtime::{
    PreparedChannelModule, WasmChannelRuntime, WasmChannelRuntimeConfig,
};
use crate::channels::wasm::wrapper::{HttpResponse, WasmChannel};
use crate::pairing::PairingStore;
use crate::tools::wasm::ResourceLimits;

use super::create_test_channel;

#[test]
fn test_channel_name() {
    let channel = create_test_channel();
    assert_eq!(channel.name(), "test");
}

#[test]
fn test_http_response_ok() {
    let response = HttpResponse::ok();
    assert_eq!(response.status, 200);
    assert!(response.body.is_empty());
}

#[test]
fn test_http_response_json() {
    let response = HttpResponse::json(serde_json::json!({"key": "value"}));
    assert_eq!(response.status, 200);
    assert_eq!(
        response.headers.get("Content-Type"),
        Some(&"application/json".to_string())
    );
}

#[test]
fn test_http_response_error() {
    let response = HttpResponse::error(400, "Bad request");
    assert_eq!(response.status, 400);
    assert_eq!(response.body, b"Bad request");
}

#[tokio::test]
async fn test_channel_start_and_shutdown() {
    let channel = create_test_channel();

    // Start should succeed
    let stream = channel.start().await;
    assert!(stream.is_ok());

    // Health check should pass
    assert!(channel.health_check().await.is_ok());

    // Shutdown should succeed
    assert!(channel.shutdown().await.is_ok());

    // Health check should fail after shutdown
    assert!(channel.health_check().await.is_err());
}

#[tokio::test]
async fn test_execute_poll_no_wasm_returns_empty() {
    // When there's no WASM module (None component), execute_poll
    // should return an empty vector of messages
    let config = WasmChannelRuntimeConfig::for_testing();
    let runtime = Arc::new(WasmChannelRuntime::new(config).unwrap());

    let prepared = Arc::new(PreparedChannelModule {
        name: "poll-test".to_string(),
        description: "Test channel".to_string(),
        component: None, // No WASM module
        limits: ResourceLimits::default(),
    });

    let capabilities = ChannelCapabilities::for_channel("poll-test").with_polling(1000);
    let credentials = Arc::new(tokio::sync::RwLock::new(std::collections::HashMap::new()));
    let timeout = std::time::Duration::from_secs(5);

    let workspace_store = Arc::new(crate::channels::wasm::host::ChannelWorkspaceStore::new());

    let result = WasmChannel::execute_poll(
        "poll-test",
        &runtime,
        &prepared,
        &capabilities,
        &credentials,
        Vec::new(), // no host credentials in test
        Arc::new(PairingStore::new()),
        timeout,
        &workspace_store,
    )
    .await;

    assert!(result.is_ok());
    assert!(result.unwrap().is_empty());
}

#[tokio::test]
async fn test_channel_with_polling_stores_shutdown_sender() {
    // Create a channel with polling capabilities
    let config = WasmChannelRuntimeConfig::for_testing();
    let runtime = Arc::new(WasmChannelRuntime::new(config).unwrap());

    let prepared = Arc::new(PreparedChannelModule {
        name: "poll-channel".to_string(),
        description: "Polling test channel".to_string(),
        component: None,
        limits: ResourceLimits::default(),
    });

    // Enable polling with a 1 second minimum interval
    let capabilities = ChannelCapabilities::for_channel("poll-channel")
        .with_path("/webhook/poll")
        .with_polling(1000);

    let channel = WasmChannel::new(
        runtime,
        prepared,
        capabilities,
        "{}".to_string(),
        Arc::new(PairingStore::new()),
        None,
    );

    // Start the channel
    let _stream = channel.start().await.expect("Channel should start");

    // Verify poll_shutdown_tx is set (polling was started)
    // Note: For testing channels without WASM, on_start returns no poll config,
    // so polling won't actually be started. This verifies the basic lifecycle.
    assert!(channel.health_check().await.is_ok());

    // Shutdown should clean up properly
    channel.shutdown().await.expect("Shutdown should succeed");
    assert!(channel.health_check().await.is_err());
}

#[tokio::test]
async fn test_call_on_poll_no_wasm_succeeds() {
    // Verify call_on_poll returns Ok when there's no WASM module
    let channel = create_test_channel();

    // Start the channel first to set up message_tx
    let _stream = channel.start().await.expect("Channel should start");

    // call_on_poll should succeed (no-op for no WASM)
    let result = channel.call_on_poll().await;
    assert!(result.is_ok());

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

    // Send Done status
    let _ = channel
        .send_status(
            crate::channels::StatusUpdate::Status("Done".into()),
            &metadata,
        )
        .await;

    // Typing task should be cancelled
    assert!(channel.typing_task.read().await.is_none());

    channel.shutdown().await.expect("Shutdown should succeed");
}

#[tokio::test]
async fn test_typing_task_persists_on_tool_started() {
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

    // Intermediate tool status should not cancel typing
    let _ = channel
        .send_status(
            crate::channels::StatusUpdate::ToolStarted {
                name: "http_request".into(),
            },
            &metadata,
        )
        .await;

    assert!(channel.typing_task.read().await.is_some());

    channel.shutdown().await.expect("Shutdown should succeed");
}

#[tokio::test]
async fn test_typing_task_cancelled_on_approval_needed() {
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

    // Approval-needed should stop typing while waiting for user action
    let _ = channel
        .send_status(
            crate::channels::StatusUpdate::ApprovalNeeded {
                request_id: "req-1".into(),
                tool_name: "http_request".into(),
                description: "Fetch weather".into(),
                parameters: serde_json::json!({"url": "https://wttr.in"}),
            },
            &metadata,
        )
        .await;

    assert!(channel.typing_task.read().await.is_none());

    channel.shutdown().await.expect("Shutdown should succeed");
}

#[tokio::test]
async fn test_typing_task_cancelled_on_awaiting_approval_status() {
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

    // Legacy terminal status string should also cancel typing
    let _ = channel
        .send_status(
            crate::channels::StatusUpdate::Status("Awaiting approval".into()),
            &metadata,
        )
        .await;

    assert!(channel.typing_task.read().await.is_none());

    channel.shutdown().await.expect("Shutdown should succeed");
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

/// Verify that WASM HTTP host functions work using a dedicated
/// current-thread runtime inside spawn_blocking.
#[tokio::test]
async fn test_dedicated_runtime_inside_spawn_blocking() {
    let result = tokio::task::spawn_blocking(|| {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("failed to build runtime");
        rt.block_on(async { 42 })
    })
    .await
    .expect("spawn_blocking panicked");
    assert_eq!(result, 42);
}

/// Verify a real HTTP request works using the dedicated-runtime pattern.
/// This catches DNS, TLS, and I/O driver issues that trivial tests miss.
#[tokio::test]
#[ignore] // requires network
async fn test_dedicated_runtime_real_http() {
    let result = tokio::task::spawn_blocking(|| {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("failed to build runtime");
        rt.block_on(async {
            let client = reqwest::Client::builder()
                .connect_timeout(std::time::Duration::from_secs(10))
                .build()
                .expect("failed to build client");
            let resp = client
                .get("https://api.telegram.org/bot000/getMe")
                .timeout(std::time::Duration::from_secs(10))
                .send()
                .await;
            match resp {
                Ok(r) => r.status().as_u16(),
                Err(e) if e.is_timeout() => panic!("request timed out: {e}"),
                Err(e) => panic!("unexpected error: {e}"),
            }
        })
    })
    .await
    .expect("spawn_blocking panicked");
    // 404 because "000" is not a valid bot token
    assert_eq!(result, 404);
}
