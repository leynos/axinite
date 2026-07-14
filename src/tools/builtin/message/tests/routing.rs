//! Tests for tool metadata, schema shape, channel/target resolution,
//! and job-metadata fallback routing.

use std::sync::Arc;

use crate::channels::ChannelManager;
use crate::tools::builtin::message::MessageTool;
use crate::tools::tool::{ApprovalRequirement, NativeTool};

#[test]
fn message_tool_name() {
    let tool = MessageTool::new(Arc::new(ChannelManager::new()));
    assert_eq!(tool.name(), "message");
}

#[test]
fn message_tool_description() {
    let tool = MessageTool::new(Arc::new(ChannelManager::new()));
    assert!(!tool.description().is_empty());
}

#[test]
fn message_tool_schema_has_required_fields() {
    let tool = MessageTool::new(Arc::new(ChannelManager::new()));
    let schema = tool.parameters_schema();

    let params = schema.get("properties").unwrap();
    assert!(params.get("content").is_some());
    assert!(params.get("channel").is_some());
    assert!(params.get("target").is_some());

    // Only content is required - channel and target can be inferred from conversation context
    let required = schema.get("required").unwrap().as_array().unwrap();
    assert!(required.iter().any(|v| v == "content"));
    assert!(!required.iter().any(|v| v == "channel"));
    assert!(!required.iter().any(|v| v == "target"));
}

#[test]
fn message_tool_schema_has_optional_attachments() {
    let tool = MessageTool::new(Arc::new(ChannelManager::new()));
    let schema = tool.parameters_schema();

    let params = schema.get("properties").unwrap();
    assert!(params.get("attachments").is_some());
}

#[tokio::test]
async fn message_tool_set_context_updates_defaults() {
    let tool = MessageTool::new(Arc::new(ChannelManager::new()));

    // Initially no defaults set
    let ctx = crate::context::JobContext::new("test", "test description");
    let result = tool
        .execute(serde_json::json!({"content": "hello"}), &ctx)
        .await;
    assert!(result.is_err()); // Should fail without defaults

    // Set context
    tool.set_context(Some("signal".to_string()), Some("+1234567890".to_string()))
        .await;

    // Now execute should use the defaults (though it will fail because channel doesn't exist)
    let result = tool
        .execute(serde_json::json!({"content": "hello"}), &ctx)
        .await;
    // Will fail because channel doesn't exist, but should attempt to use the defaults
    assert!(result.is_err());
    let err = result.unwrap_err().to_string();
    assert!(err.contains("signal") || err.contains("No channels connected"));
}

#[tokio::test]
async fn message_tool_explicit_params_override_defaults() {
    let tool = MessageTool::new(Arc::new(ChannelManager::new()));

    // Set defaults
    tool.set_context(Some("signal".to_string()), Some("+1234567890".to_string()))
        .await;

    // Execute with explicit params - should fail but check that it uses explicit params
    let ctx = crate::context::JobContext::new("test", "test description");
    let result = tool
        .execute(
            serde_json::json!({
                "content": "hello",
                "channel": "telegram",
                "target": "@username"
            }),
            &ctx,
        )
        .await;

    // Will fail because channel doesn't exist
    assert!(result.is_err());
    let err = result.unwrap_err().to_string();
    // Should reference telegram, not signal
    assert!(err.contains("telegram") || err.contains("No channels connected"));
}

#[tokio::test]
async fn message_tool_requires_content() {
    let tool = MessageTool::new(Arc::new(ChannelManager::new()));

    let ctx = crate::context::JobContext::new("test", "test description");
    let result = tool
        .execute(
            serde_json::json!({
                "channel": "signal",
                "target": "+1234567890"
            }),
            &ctx,
        )
        .await;

    assert!(result.is_err());
    let err = result.unwrap_err().to_string();
    assert!(err.contains("content") || err.contains("required"));
}

#[test]
fn message_tool_does_not_require_sanitization() {
    let tool = MessageTool::new(Arc::new(ChannelManager::new()));
    assert!(!tool.requires_sanitization());
}

#[test]
fn requires_approval_always_never() {
    // Message tool only sends to user-owned channels, so never needs approval.
    let tool = MessageTool::new(Arc::new(ChannelManager::new()));
    assert_eq!(
        tool.requires_approval(&serde_json::json!({"content": "hello"})),
        ApprovalRequirement::Never,
    );
    assert_eq!(
        tool.requires_approval(&serde_json::json!({"content": "hi", "channel": "telegram"})),
        ApprovalRequirement::Never,
    );
}

#[tokio::test]
async fn message_tool_falls_back_to_job_metadata() {
    // Regression: when no conversation context is set (e.g. routine full-job),
    // the message tool should fall back to notify_channel/notify_user from
    // JobContext metadata instead of returning "No target specified".
    let tool = MessageTool::new(Arc::new(ChannelManager::new()));

    let mut ctx = crate::context::JobContext::new("routine-job", "price alert");
    ctx.metadata = serde_json::json!({
        "notify_channel": "telegram",
        "notify_user": "123456789"});

    // No set_context called — simulates a routine full-job worker
    let result = tool
        .execute(serde_json::json!({"content": "NEAR price is $5"}), &ctx)
        .await;

    // Should fail at channel broadcast (no real channel), NOT at
    // "No target specified and no active conversation"
    assert!(result.is_err());
    let err = result.unwrap_err().to_string();
    assert!(
        !err.contains("No target specified"),
        "Should not get 'No target specified' when metadata has notify_user, got: {}",
        err
    );
    assert!(
        !err.contains("No channel specified"),
        "Should not get 'No channel specified' when metadata has notify_channel, got: {}",
        err
    );
}

#[tokio::test]
async fn message_tool_no_metadata_still_errors() {
    // When neither conversation context nor metadata is set, should still
    // return a clear error (target resolution fails).
    let tool = MessageTool::new(Arc::new(ChannelManager::new()));
    let ctx = crate::context::JobContext::new("orphan-job", "no notify config");

    let result = tool
        .execute(serde_json::json!({"content": "hello"}), &ctx)
        .await;

    assert!(result.is_err());
    let err = result.unwrap_err().to_string();
    assert!(
        err.contains("No target specified"),
        "Expected 'No target specified' error, got: {}",
        err
    );
}

#[tokio::test]
async fn message_tool_broadcasts_all_when_no_channel() {
    // Regression: when notify.channel is None but notify_user is set,
    // the message tool should attempt broadcast_all instead of erroring
    // with "No channel specified".
    let tool = MessageTool::new(Arc::new(ChannelManager::new()));

    let mut ctx = crate::context::JobContext::new("routine-job", "price alert");
    ctx.metadata = serde_json::json!({
        "notify_user": "123456789"});

    let result = tool
        .execute(serde_json::json!({"content": "NEAR price is $5"}), &ctx)
        .await;

    // Should fail because no channels are registered (empty ChannelManager),
    // NOT because "No channel specified".
    assert!(result.is_err());
    let err = result.unwrap_err().to_string();
    assert!(
        !err.contains("No channel specified"),
        "Should not get 'No channel specified' when broadcasting, got: {}",
        err
    );
    assert!(
        err.contains("No channels connected") || err.contains("All channels failed"),
        "Expected channel delivery error, got: {}",
        err
    );
}
