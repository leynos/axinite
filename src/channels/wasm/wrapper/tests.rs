//! Unit and integration tests for the WASM channel wrapper.

use std::sync::Arc;

use crate::channels::NativeChannel;
use crate::channels::wasm::capabilities::ChannelCapabilities;
use crate::channels::wasm::runtime::{
    PreparedChannelModule, WasmChannelRuntime, WasmChannelRuntimeConfig,
};
use crate::channels::wasm::wrapper::{HttpResponse, WasmChannel};
use crate::pairing::PairingStore;
use crate::testing::credentials::TEST_TELEGRAM_BOT_TOKEN;
use crate::tools::wasm::ResourceLimits;

use super::attachments::mime_from_extension;
use super::types::{ChannelName, HostPattern, SecretValue};

fn create_test_channel() -> WasmChannel {
    let config = WasmChannelRuntimeConfig::for_testing();
    let runtime = Arc::new(WasmChannelRuntime::new(config).unwrap());

    let prepared = Arc::new(PreparedChannelModule {
        name: "test".to_string(),
        description: "Test channel".to_string(),
        component: None,
        limits: ResourceLimits::default(),
    });

    let capabilities = ChannelCapabilities::for_channel("test").with_path("/webhook/test");

    WasmChannel::new(
        runtime,
        prepared,
        capabilities,
        "{}".to_string(),
        Arc::new(PairingStore::new()),
        None,
    )
}

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
async fn test_dispatch_emitted_messages_sends_to_channel() {
    use crate::channels::wasm::host::EmittedMessage;

    let (tx, mut rx) = tokio::sync::mpsc::channel(10);
    let message_tx = Arc::new(tokio::sync::RwLock::new(Some(tx)));

    let rate_limiter = Arc::new(tokio::sync::RwLock::new(
        crate::channels::wasm::host::ChannelEmitRateLimiter::new(
            crate::channels::wasm::capabilities::EmitRateLimitConfig::default(),
        ),
    ));

    let messages = vec![
        EmittedMessage::new("user1", "Hello from polling!"),
        EmittedMessage::new("user2", "Another message"),
    ];

    let last_broadcast_metadata = Arc::new(tokio::sync::RwLock::new(None));
    let result = WasmChannel::dispatch_emitted_messages(
        "test-channel",
        messages,
        &message_tx,
        &rate_limiter,
        &last_broadcast_metadata,
        None,
    )
    .await;

    assert!(result.is_ok());

    // Verify messages were sent
    let msg1 = rx.try_recv().expect("Should receive first message");
    assert_eq!(msg1.user_id, "user1");
    assert_eq!(msg1.content, "Hello from polling!");

    let msg2 = rx.try_recv().expect("Should receive second message");
    assert_eq!(msg2.user_id, "user2");
    assert_eq!(msg2.content, "Another message");

    // No more messages
    assert!(rx.try_recv().is_err());
}

#[tokio::test]
async fn test_dispatch_emitted_messages_no_sender_returns_ok() {
    use crate::channels::wasm::host::EmittedMessage;

    // No sender available (channel not started)
    let message_tx = Arc::new(tokio::sync::RwLock::new(None));
    let rate_limiter = Arc::new(tokio::sync::RwLock::new(
        crate::channels::wasm::host::ChannelEmitRateLimiter::new(
            crate::channels::wasm::capabilities::EmitRateLimitConfig::default(),
        ),
    ));

    let messages = vec![EmittedMessage::new("user1", "Hello!")];

    // Should return Ok even without a sender (logs warning but doesn't fail)
    let last_broadcast_metadata = Arc::new(tokio::sync::RwLock::new(None));
    let result = WasmChannel::dispatch_emitted_messages(
        "test-channel",
        messages,
        &message_tx,
        &rate_limiter,
        &last_broadcast_metadata,
        None,
    )
    .await;

    assert!(result.is_ok());
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

#[test]
fn test_status_to_wit_thinking() {
    use super::status_to_wit;

    let metadata = serde_json::json!({"chat_id": 42});
    let wit = status_to_wit(
        &crate::channels::StatusUpdate::Thinking("Processing...".into()),
        &metadata,
    );

    assert!(matches!(
        wit.status,
        super::wit_channel::StatusType::Thinking
    ));
    assert_eq!(wit.message, "Processing...");
    assert!(wit.metadata_json.contains("42"));
}

#[test]
fn test_status_to_wit_done() {
    use super::status_to_wit;

    let metadata = serde_json::json!(null);
    let wit = status_to_wit(
        &crate::channels::StatusUpdate::Status("Done".into()),
        &metadata,
    );

    assert!(matches!(wit.status, super::wit_channel::StatusType::Done));
}

#[test]
fn test_status_to_wit_done_case_insensitive() {
    use super::status_to_wit;

    let metadata = serde_json::json!(null);

    // lowercase
    let wit = status_to_wit(
        &crate::channels::StatusUpdate::Status("done".into()),
        &metadata,
    );
    assert!(matches!(wit.status, super::wit_channel::StatusType::Done));

    // with whitespace
    let wit = status_to_wit(
        &crate::channels::StatusUpdate::Status(" Done ".into()),
        &metadata,
    );
    assert!(matches!(wit.status, super::wit_channel::StatusType::Done));
}

#[test]
fn test_status_to_wit_interrupted() {
    use super::status_to_wit;

    let metadata = serde_json::json!(null);
    let wit = status_to_wit(
        &crate::channels::StatusUpdate::Status("Interrupted".into()),
        &metadata,
    );

    assert!(matches!(
        wit.status,
        super::wit_channel::StatusType::Interrupted
    ));
}

#[test]
fn test_status_to_wit_interrupted_case_insensitive() {
    use super::status_to_wit;

    let metadata = serde_json::json!(null);

    // lowercase
    let wit = status_to_wit(
        &crate::channels::StatusUpdate::Status("interrupted".into()),
        &metadata,
    );
    assert!(matches!(
        wit.status,
        super::wit_channel::StatusType::Interrupted
    ));

    // with whitespace
    let wit = status_to_wit(
        &crate::channels::StatusUpdate::Status(" Interrupted ".into()),
        &metadata,
    );
    assert!(matches!(
        wit.status,
        super::wit_channel::StatusType::Interrupted
    ));
}

#[test]
fn test_status_to_wit_generic_status() {
    use super::status_to_wit;

    let metadata = serde_json::json!(null);
    let wit = status_to_wit(
        &crate::channels::StatusUpdate::Status("Awaiting approval".into()),
        &metadata,
    );

    assert!(matches!(wit.status, super::wit_channel::StatusType::Status));
    assert_eq!(wit.message, "Awaiting approval");
}

#[test]
fn test_status_to_wit_auth_required() {
    use super::status_to_wit;

    let metadata = serde_json::json!({"chat_id": 42});
    let wit = status_to_wit(
        &crate::channels::StatusUpdate::AuthRequired {
            extension_name: "weather".to_string(),
            instructions: Some("Paste your token".to_string()),
            auth_url: Some("https://example.com/auth".to_string()),
            setup_url: None,
        },
        &metadata,
    );

    assert!(matches!(
        wit.status,
        super::wit_channel::StatusType::AuthRequired
    ));
    assert!(wit.message.contains("Authentication required for weather"));
    assert!(wit.message.contains("Paste your token"));
}

#[test]
fn test_status_to_wit_tool_started() {
    use super::status_to_wit;

    let metadata = serde_json::json!({"chat_id": 7});
    let wit = status_to_wit(
        &crate::channels::StatusUpdate::ToolStarted {
            name: "http_request".to_string(),
        },
        &metadata,
    );

    assert!(matches!(
        wit.status,
        super::wit_channel::StatusType::ToolStarted
    ));
    assert_eq!(wit.message, "Tool started: http_request");
}

#[test]
fn test_status_to_wit_tool_completed_success() {
    use super::status_to_wit;

    let metadata = serde_json::json!(null);
    let wit = status_to_wit(
        &crate::channels::StatusUpdate::ToolCompleted {
            name: "http_request".to_string(),
            success: true,
            error: None,
            parameters: None,
        },
        &metadata,
    );

    assert!(matches!(
        wit.status,
        super::wit_channel::StatusType::ToolCompleted
    ));
    assert_eq!(wit.message, "Tool completed: http_request (ok)");
}

#[test]
fn test_status_to_wit_tool_completed_failure() {
    use super::status_to_wit;

    let metadata = serde_json::json!(null);
    let wit = status_to_wit(
        &crate::channels::StatusUpdate::ToolCompleted {
            name: "http_request".to_string(),
            success: false,
            error: Some("connection refused".to_string()),
            parameters: None,
        },
        &metadata,
    );

    assert!(matches!(
        wit.status,
        super::wit_channel::StatusType::ToolCompleted
    ));
    assert_eq!(wit.message, "Tool completed: http_request (failed)");
}

#[test]
fn test_status_to_wit_tool_result() {
    use super::status_to_wit;

    let metadata = serde_json::json!(null);
    let wit = status_to_wit(
        &crate::channels::StatusUpdate::ToolResult {
            name: "http_request".to_string(),
            preview: "{".to_string() + "\"temperature\": 22}",
        },
        &metadata,
    );

    assert!(matches!(
        wit.status,
        super::wit_channel::StatusType::ToolResult
    ));
    assert!(wit.message.starts_with("Tool result: http_request\n"));
}

#[test]
fn test_status_to_wit_tool_result_truncates_preview() {
    use super::status_to_wit;

    let metadata = serde_json::json!(null);
    let long_preview = "x".repeat(400);
    let wit = status_to_wit(
        &crate::channels::StatusUpdate::ToolResult {
            name: "big_tool".to_string(),
            preview: long_preview,
        },
        &metadata,
    );

    assert!(matches!(
        wit.status,
        super::wit_channel::StatusType::ToolResult
    ));
    assert!(wit.message.ends_with("..."));
}

#[test]
fn test_status_to_wit_job_started() {
    use super::status_to_wit;

    let metadata = serde_json::json!({"chat_id": 1});
    let wit = status_to_wit(
        &crate::channels::StatusUpdate::JobStarted {
            job_id: "job-1".to_string(),
            title: "Daily sync".to_string(),
            browse_url: "https://example.com/jobs/job-1".to_string(),
        },
        &metadata,
    );

    assert!(matches!(
        wit.status,
        super::wit_channel::StatusType::JobStarted
    ));
    assert!(wit.message.contains("Daily sync"));
    assert!(wit.message.contains("https://example.com/jobs/job-1"));
}

#[test]
fn test_status_to_wit_auth_completed_success() {
    use super::status_to_wit;

    let metadata = serde_json::json!(null);
    let wit = status_to_wit(
        &crate::channels::StatusUpdate::AuthCompleted {
            extension_name: "weather".to_string(),
            success: true,
            message: "Token saved".to_string(),
        },
        &metadata,
    );

    assert!(matches!(
        wit.status,
        super::wit_channel::StatusType::AuthCompleted
    ));
    assert!(wit.message.contains("Authentication completed"));
    assert!(wit.message.contains("Token saved"));
}

#[test]
fn test_status_to_wit_auth_completed_failure() {
    use super::status_to_wit;

    let metadata = serde_json::json!(null);
    let wit = status_to_wit(
        &crate::channels::StatusUpdate::AuthCompleted {
            extension_name: "weather".to_string(),
            success: false,
            message: "Invalid token".to_string(),
        },
        &metadata,
    );

    assert!(matches!(
        wit.status,
        super::wit_channel::StatusType::AuthCompleted
    ));
    assert!(wit.message.contains("Authentication failed"));
    assert!(wit.message.contains("Invalid token"));
}

#[test]
fn test_status_to_wit_approval_needed() {
    use super::status_to_wit;

    let metadata = serde_json::json!({"chat_id": 42});
    let wit = status_to_wit(
        &crate::channels::StatusUpdate::ApprovalNeeded {
            request_id: "req-123".to_string(),
            tool_name: "http_request".to_string(),
            description: "Fetch weather data".to_string(),
            parameters: serde_json::json!({"url": "https://api.weather.test"}),
        },
        &metadata,
    );

    assert!(matches!(
        wit.status,
        super::wit_channel::StatusType::ApprovalNeeded
    ));
    assert!(wit.message.contains("http_request"));
    assert!(wit.message.contains("/approve"));
}

#[test]
fn test_approval_prompt_roundtrip_submission_aliases() {
    use super::status_to_wit;
    use crate::agent::submission::{Submission, SubmissionParser};

    let metadata = serde_json::json!({"chat_id": 42});
    let wit = status_to_wit(
        &crate::channels::StatusUpdate::ApprovalNeeded {
            request_id: "req-321".to_string(),
            tool_name: "http_request".to_string(),
            description: "Fetch weather data".to_string(),
            parameters: serde_json::json!({"url": "https://api.weather.test"}),
        },
        &metadata,
    );

    assert!(matches!(
        wit.status,
        super::wit_channel::StatusType::ApprovalNeeded
    ));
    assert!(wit.message.contains("/approve"));
    assert!(wit.message.contains("/deny"));
    assert!(wit.message.contains("/always"));

    let approve = SubmissionParser::parse("/approve");
    assert!(matches!(
        approve,
        Submission::ApprovalResponse {
            approved: true,
            always: false
        }
    ));

    let deny = SubmissionParser::parse("/deny");
    assert!(matches!(
        deny,
        Submission::ApprovalResponse {
            approved: false,
            always: false
        }
    ));

    let always = SubmissionParser::parse("/always");
    assert!(matches!(
        always,
        Submission::ApprovalResponse {
            approved: true,
            always: true
        }
    ));
}

#[test]
fn test_clone_wit_status_update() {
    use super::{clone_wit_status_update, wit_channel};

    let original = wit_channel::StatusUpdate {
        status: wit_channel::StatusType::Thinking,
        message: "hello".to_string(),
        metadata_json: "{\"a\":1}".to_string(),
    };

    let cloned = clone_wit_status_update(&original);
    assert!(matches!(cloned.status, wit_channel::StatusType::Thinking));
    assert_eq!(cloned.message, "hello");
    assert_eq!(cloned.metadata_json, "{\"a\":1}");
}

#[test]
fn test_clone_wit_status_update_approval_needed() {
    use super::{clone_wit_status_update, wit_channel};

    let original = wit_channel::StatusUpdate {
        status: wit_channel::StatusType::ApprovalNeeded,
        message: "approval needed".to_string(),
        metadata_json: "{\"chat_id\":42}".to_string(),
    };

    let cloned = clone_wit_status_update(&original);
    assert!(matches!(
        cloned.status,
        wit_channel::StatusType::ApprovalNeeded
    ));
    assert_eq!(cloned.message, "approval needed");
    assert_eq!(cloned.metadata_json, "{\"chat_id\":42}");
}

#[test]
fn test_clone_wit_status_update_auth_completed() {
    use super::{clone_wit_status_update, wit_channel};

    let original = wit_channel::StatusUpdate {
        status: wit_channel::StatusType::AuthCompleted,
        message: "auth complete".to_string(),
        metadata_json: "{}".to_string(),
    };

    let cloned = clone_wit_status_update(&original);
    assert!(matches!(
        cloned.status,
        wit_channel::StatusType::AuthCompleted
    ));
    assert_eq!(cloned.message, "auth complete");
}

#[test]
fn test_clone_wit_status_update_all_variants() {
    use super::{clone_wit_status_update, wit_channel};

    let variants = vec![
        wit_channel::StatusType::Thinking,
        wit_channel::StatusType::Done,
        wit_channel::StatusType::Interrupted,
        wit_channel::StatusType::ToolStarted,
        wit_channel::StatusType::ToolCompleted,
        wit_channel::StatusType::ToolResult,
        wit_channel::StatusType::ApprovalNeeded,
        wit_channel::StatusType::Status,
        wit_channel::StatusType::JobStarted,
        wit_channel::StatusType::AuthRequired,
        wit_channel::StatusType::AuthCompleted,
    ];

    for status in variants {
        let original = wit_channel::StatusUpdate {
            status,
            message: "sample".to_string(),
            metadata_json: "{}".to_string(),
        };
        let cloned = clone_wit_status_update(&original);

        assert_eq!(
            std::mem::discriminant(&cloned.status),
            std::mem::discriminant(&original.status)
        );
        assert_eq!(cloned.message, "sample");
        assert_eq!(cloned.metadata_json, "{}");
    }
}

#[test]
fn test_redact_credentials_replaces_values() {
    use super::ChannelStoreData;

    let mut creds = std::collections::HashMap::new();
    creds.insert(
        "TELEGRAM_BOT_TOKEN".to_string(),
        SecretValue::new(TEST_TELEGRAM_BOT_TOKEN.to_string()),
    );
    creds.insert("OTHER_SECRET".to_string(), SecretValue::new("s3cret"));
    let channel_name = ChannelName::new("test").expect("test channel name is non-empty");

    let store = ChannelStoreData::new(
        1024 * 1024,
        &channel_name,
        ChannelCapabilities::default(),
        creds,
        Vec::new(),
        Arc::new(PairingStore::new()),
    );

    let error = format!(
        "HTTP request failed: error sending request for url \
            (https://api.telegram.org/bot{TEST_TELEGRAM_BOT_TOKEN}/getUpdates)"
    );

    let redacted = store.redact_credentials(&error);

    assert!(
        !redacted.contains(TEST_TELEGRAM_BOT_TOKEN),
        "credential value should be redacted"
    );
    assert!(
        redacted.contains("[REDACTED:TELEGRAM_BOT_TOKEN]"),
        "redacted text should contain placeholder name"
    );
    assert!(
        !redacted.contains("s3cret"),
        "other credentials should also be redacted"
    );
}

#[test]
fn test_redact_credentials_no_op_without_credentials() {
    use super::ChannelStoreData;
    let channel_name = ChannelName::new("test").expect("test channel name is non-empty");

    let store = ChannelStoreData::new(
        1024 * 1024,
        &channel_name,
        ChannelCapabilities::default(),
        std::collections::HashMap::new(),
        Vec::new(),
        Arc::new(PairingStore::new()),
    );

    let input = "some error message";
    assert_eq!(store.redact_credentials(input), input);
}

#[test]
fn test_redact_credentials_url_encoded() {
    use super::{ChannelStoreData, ResolvedHostCredential};

    // Credential with characters that get URL-encoded
    let mut creds = std::collections::HashMap::new();
    creds.insert(
        "API_KEY".to_string(),
        SecretValue::new("key with spaces&special=chars"),
    );

    let host_creds = vec![ResolvedHostCredential {
        host_patterns: vec![
            HostPattern::new("api.example.com").expect("test host pattern is non-empty"),
        ],
        headers: std::collections::HashMap::new(),
        query_params: std::collections::HashMap::new(),
        secret_value: SecretValue::new("host secret+value"),
    }];
    let channel_name = ChannelName::new("test").expect("test channel name is non-empty");

    let store = ChannelStoreData::new(
        1024 * 1024,
        &channel_name,
        ChannelCapabilities::default(),
        creds,
        host_creds,
        Arc::new(PairingStore::new()),
    );

    // Error containing URL-encoded form of the credential
    let error = "request failed: https://api.example.com?key=key%20with%20spaces%26special%3Dchars&host=host%20secret%2Bvalue";

    let redacted = store.redact_credentials(error);

    assert!(
        !redacted.contains("key%20with%20spaces"),
        "URL-encoded credential should be redacted, got: {}",
        redacted
    );
    assert!(
        !redacted.contains("host%20secret%2Bvalue"),
        "URL-encoded host credential should be redacted, got: {}",
        redacted
    );
}

#[test]
fn test_redact_credentials_skips_empty_values() {
    use super::ChannelStoreData;

    let mut creds = std::collections::HashMap::new();
    creds.insert("EMPTY_TOKEN".to_string(), SecretValue::new(String::new()));
    let channel_name = ChannelName::new("test").expect("test channel name is non-empty");

    let store = ChannelStoreData::new(
        1024 * 1024,
        &channel_name,
        ChannelCapabilities::default(),
        creds,
        Vec::new(),
        Arc::new(PairingStore::new()),
    );

    let input = "should not match anything";
    assert_eq!(store.redact_credentials(input), input);
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

#[tokio::test]
async fn test_dispatch_emitted_messages_preserves_attachments() {
    use crate::channels::wasm::host::{Attachment, EmittedMessage};

    let (tx, mut rx) = tokio::sync::mpsc::channel(10);
    let message_tx = Arc::new(tokio::sync::RwLock::new(Some(tx)));

    let rate_limiter = Arc::new(tokio::sync::RwLock::new(
        crate::channels::wasm::host::ChannelEmitRateLimiter::new(
            crate::channels::wasm::capabilities::EmitRateLimitConfig::default(),
        ),
    ));

    let attachments = vec![
        Attachment {
            id: "photo123".to_string(),
            mime_type: "image/jpeg".to_string(),
            filename: Some("cat.jpg".to_string()),
            size_bytes: Some(50_000),
            source_url: Some("https://api.telegram.org/file/photo123".to_string()),
            storage_key: None,
            extracted_text: None,
            data: Vec::new(),
            duration_secs: None,
        },
        Attachment {
            id: "doc456".to_string(),
            mime_type: "application/pdf".to_string(),
            filename: Some("report.pdf".to_string()),
            size_bytes: Some(120_000),
            source_url: None,
            storage_key: Some("store/doc456".to_string()),
            extracted_text: Some("Report contents...".to_string()),
            data: Vec::new(),
            duration_secs: None,
        },
    ];

    let messages =
        vec![EmittedMessage::new("user1", "Check these files").with_attachments(attachments)];

    let last_broadcast_metadata = Arc::new(tokio::sync::RwLock::new(None));
    let result = WasmChannel::dispatch_emitted_messages(
        "test-channel",
        messages,
        &message_tx,
        &rate_limiter,
        &last_broadcast_metadata,
        None,
    )
    .await;

    assert!(result.is_ok());

    let msg = rx.try_recv().expect("Should receive message");
    assert_eq!(msg.content, "Check these files");
    assert_eq!(msg.attachments.len(), 2);

    // Verify first attachment
    assert_eq!(msg.attachments[0].id, "photo123");
    assert_eq!(msg.attachments[0].mime_type, "image/jpeg");
    assert_eq!(msg.attachments[0].filename, Some("cat.jpg".to_string()));
    assert_eq!(msg.attachments[0].size_bytes, Some(50_000));
    assert_eq!(
        msg.attachments[0].source_url,
        Some("https://api.telegram.org/file/photo123".to_string())
    );

    // Verify second attachment
    assert_eq!(msg.attachments[1].id, "doc456");
    assert_eq!(msg.attachments[1].mime_type, "application/pdf");
    assert_eq!(
        msg.attachments[1].extracted_text,
        Some("Report contents...".to_string())
    );
    assert_eq!(
        msg.attachments[1].storage_key,
        Some("store/doc456".to_string())
    );
}

#[tokio::test]
async fn test_dispatch_emitted_messages_no_attachments_backward_compat() {
    use crate::channels::wasm::host::EmittedMessage;

    let (tx, mut rx) = tokio::sync::mpsc::channel(10);
    let message_tx = Arc::new(tokio::sync::RwLock::new(Some(tx)));

    let rate_limiter = Arc::new(tokio::sync::RwLock::new(
        crate::channels::wasm::host::ChannelEmitRateLimiter::new(
            crate::channels::wasm::capabilities::EmitRateLimitConfig::default(),
        ),
    ));

    let messages = vec![EmittedMessage::new("user1", "Just text, no attachments")];

    let last_broadcast_metadata = Arc::new(tokio::sync::RwLock::new(None));
    let result = WasmChannel::dispatch_emitted_messages(
        "test-channel",
        messages,
        &message_tx,
        &rate_limiter,
        &last_broadcast_metadata,
        None,
    )
    .await;

    assert!(result.is_ok());

    let msg = rx.try_recv().expect("Should receive message");
    assert_eq!(msg.content, "Just text, no attachments");
    assert!(msg.attachments.is_empty());
}

fn test_channel_http_capabilities(host: &str) -> ChannelCapabilities {
    use crate::tools::wasm::{Capabilities, EndpointPattern, HttpCapability};

    ChannelCapabilities::for_channel("test").with_tool_capabilities(
        Capabilities::default().with_http(HttpCapability::new(vec![
            EndpointPattern::host(host.to_string())
                .with_path_prefix("/")
                .with_methods(vec!["GET".to_string()]),
        ])),
    )
}

#[test]
fn test_channel_http_request_allows_placeholder_header_injection() {
    use crate::channels::wasm::wrapper::ChannelStoreData;
    use crate::channels::wasm::wrapper::near::agent::channel_host;
    use std::collections::HashMap;

    let host = "slack.invalid";
    let slack_bot_token = "slack-dummy-token-12345".to_string();
    let mut credentials = HashMap::new();
    credentials.insert(
        "SLACK_BOT_TOKEN".to_string(),
        SecretValue::new(slack_bot_token),
    );
    let channel_name = ChannelName::new("test").expect("test channel name is non-empty");

    let mut store = ChannelStoreData::new(
        1024 * 1024,
        &channel_name,
        test_channel_http_capabilities(host),
        credentials,
        Vec::new(),
        Arc::new(PairingStore::new()),
    );

    let err = <ChannelStoreData as channel_host::Host>::http_request(
        &mut store,
        channel_host::HttpRequestParams {
            method: "GET".to_string(),
            url: format!("https://{host}/api/chat.postMessage"),
            headers_json: serde_json::json!({
                "Authorization": "Bearer {SLACK_BOT_TOKEN}",
                "Content-Type": "application/json"
            })
            .to_string(),
            body: None,
            timeout_ms: Some(1000),
        },
    )
    .expect_err("invalid public hostname should fail after request preparation");

    assert!(
        !err.contains("Potential secret leak blocked"),
        "placeholder-based auth header should progress past leak scanning, got: {err}"
    );
    assert!(
        err.contains("HTTP request failed") || err.contains("dns error"),
        "expected later-stage HTTP/DNS failure, got: {err}"
    );
}

#[test]
fn test_mime_from_extension() {
    assert_eq!(mime_from_extension("screenshot.png"), "image/png");
    assert_eq!(mime_from_extension("photo.JPG"), "image/jpeg");
    assert_eq!(mime_from_extension("photo.jpeg"), "image/jpeg");
    assert_eq!(mime_from_extension("animation.gif"), "image/gif");
    assert_eq!(mime_from_extension("doc.pdf"), "application/pdf");
    assert_eq!(mime_from_extension("video.mp4"), "video/mp4");
    assert_eq!(mime_from_extension("data.csv"), "text/csv");
    assert_eq!(
        mime_from_extension("unknown.qqqzzz"),
        "application/octet-stream"
    );
    assert_eq!(mime_from_extension("noext"), "application/octet-stream");
    assert_eq!(
        mime_from_extension("/home/user/.ironclaw/screenshot.png"),
        "image/png"
    );
}
