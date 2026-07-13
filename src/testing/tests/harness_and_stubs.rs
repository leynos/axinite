//! Tests for the harness builder defaults and the LLM/channel stubs.

#[cfg(all(feature = "libsql", feature = "test-helpers"))]
use std::sync::Arc;

use crate::channels::{IncomingMessage, NativeChannel, OutgoingResponse};
#[cfg(all(feature = "libsql", feature = "test-helpers"))]
use crate::llm::{CompletionRequest, FinishReason, LlmProvider};
#[cfg(all(feature = "libsql", feature = "test-helpers"))]
use crate::testing::TestHarnessBuilder;
use crate::testing::{StubChannel, StubLlm};

#[cfg(all(feature = "libsql", feature = "test-helpers"))]
#[tokio::test]
async fn test_harness_builds_with_defaults() {
    let harness = TestHarnessBuilder::new()
        .build()
        .await
        .expect("test harness should build");
    assert!(harness.deps.store.is_some());
    assert_eq!(harness.deps.llm.model_name(), "stub-model");
}

#[cfg(all(feature = "libsql", feature = "test-helpers"))]
#[tokio::test]
async fn test_harness_custom_llm() {
    let custom_llm = Arc::new(StubLlm::new("custom response").with_model_name("my-model"));
    let harness = TestHarnessBuilder::new()
        .with_llm(custom_llm)
        .build()
        .await
        .expect("test harness should build");
    assert_eq!(harness.deps.llm.model_name(), "my-model");
}

#[cfg(all(feature = "libsql", feature = "test-helpers"))]
#[tokio::test]
async fn test_harness_db_works() {
    let harness = TestHarnessBuilder::new()
        .build()
        .await
        .expect("test harness should build");

    let id = harness
        .db
        .create_conversation("test", "user1", None)
        .await
        .expect("create conversation");
    assert!(!id.is_nil());
}

#[tokio::test]
async fn test_stub_llm_complete() {
    let llm = StubLlm::new("hello world");
    let response = llm
        .complete(CompletionRequest::new(vec![]))
        .await
        .expect("complete");
    assert_eq!(response.content, "hello world");
    assert_eq!(response.finish_reason, FinishReason::Stop);
}

#[tokio::test]
async fn test_stub_channel_inject_and_capture() {
    use futures::StreamExt;

    let (channel, sender) = StubChannel::new("test-channel");

    // Start the channel to get the message stream
    let mut stream = channel.start().await.expect("start failed");

    // Inject a message
    sender
        .send(IncomingMessage::new("test-channel", "user1", "hello"))
        .await
        .expect("send failed");

    // Read it from the stream
    let msg = stream.next().await.expect("stream ended");
    assert_eq!(msg.content, "hello");
    assert_eq!(msg.user_id, "user1");
    assert_eq!(msg.channel, "test-channel");

    // Send a response and verify it was captured
    let response = OutgoingResponse::text("world");
    channel
        .respond(&msg, response)
        .await
        .expect("respond failed");

    let captured = channel.captured_responses();
    assert_eq!(captured.len(), 1);
    assert_eq!(captured[0].1.content, "world");
}

#[tokio::test]
async fn test_stub_channel_health_check() {
    let (channel, _sender) = StubChannel::new("healthy");
    channel.health_check().await.expect("health check failed");

    channel.set_healthy(false);
    assert!(channel.health_check().await.is_err());
}

#[cfg(all(feature = "libsql", feature = "test-helpers"))]
#[tokio::test]
async fn test_harness_with_channel() {
    let harness = TestHarnessBuilder::new()
        .with_stub_channel()
        .build()
        .await
        .expect("test harness should build");

    let (sender, channel_manager) = harness.channel.as_ref().expect("channel should be present");

    // Inject a message via sender
    sender
        .send(IncomingMessage::new("stub", "user1", "test message"))
        .await
        .expect("send failed");

    // Verify channel is registered in the manager
    let names = channel_manager.channel_names().await;
    assert!(names.contains(&"stub".to_string()));
}
