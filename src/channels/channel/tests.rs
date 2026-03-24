//! Tests for channel trait and message types.

use super::*;
use crate::testing::credentials::TEST_REDACT_SECRET_123;
use rstest::rstest;

/// Stub tool that marks `"value"` as sensitive.
struct SecretTool;

impl crate::tools::NativeTool for SecretTool {
    fn name(&self) -> &str {
        "secret_save"
    }
    fn description(&self) -> &str {
        "stub"
    }
    fn parameters_schema(&self) -> serde_json::Value {
        serde_json::json!({"type": "object", "properties": {}})
    }
    async fn execute(
        &self,
        _params: serde_json::Value,
        _ctx: &crate::context::JobContext,
    ) -> Result<crate::tools::ToolOutput, crate::tools::ToolError> {
        unreachable!()
    }
    fn sensitive_params(&self) -> &[&str] {
        &["value"]
    }
}

/// Parameterized tests for StatusUpdate::tool_completed behavior.
#[rstest]
#[case::failure_with_redaction(
    "secret_save",
    serde_json::json!({"name": "api_key", "value": TEST_REDACT_SECRET_123}),
    Err(crate::error::ToolError::ExecutionFailed {
        name: "secret_save".into(),
        reason: "db error".into(),
    }.into()),
    true, // has_tool
    false, // expected_success
    true, // should_have_params
    Some("[REDACTED]"), // must_contain
    Some(TEST_REDACT_SECRET_123), // must_not_contain
    Some("api_key") // additional_check
)]
#[case::success_no_params(
    "secret_save",
    serde_json::json!({"name": "key", "value": "secret"}),
    Ok("done".to_string()),
    false, // has_tool
    true, // expected_success
    false, // should_have_params
    None,
    None,
    None
)]
#[case::failure_no_tool_unredacted(
    "shell",
    serde_json::json!({"cmd": "ls -la"}),
    Err(crate::error::ToolError::ExecutionFailed {
        name: "shell".into(),
        reason: "timeout".into(),
    }.into()),
    false, // has_tool
    false, // expected_success
    true, // should_have_params
    Some("ls -la"), // must_contain
    None,
    None
)]
fn tool_completed_parameterized(
    #[case] tool_name: &str,
    #[case] params: serde_json::Value,
    #[case] result: Result<String, crate::error::Error>,
    #[case] has_tool: bool,
    #[case] expected_success: bool,
    #[case] should_have_params: bool,
    #[case] must_contain: Option<&str>,
    #[case] must_not_contain: Option<&str>,
    #[case] additional_check: Option<&str>,
) {
    let tool_inst = SecretTool;
    let tool_ref: Option<&dyn crate::tools::Tool> = if has_tool {
        Some(&tool_inst as &dyn crate::tools::Tool)
    } else {
        None
    };

    let status = StatusUpdate::tool_completed(tool_name.into(), &result, &params, tool_ref);

    if let StatusUpdate::ToolCompleted {
        success,
        error,
        parameters,
        ..
    } = &status
    {
        assert_eq!(*success, expected_success);

        if expected_success {
            assert!(error.is_none());
        } else {
            assert!(error.is_some());
        }

        if should_have_params {
            let param_str = parameters
                .as_ref()
                .expect("should have parameters when expected");
            if let Some(must_have) = must_contain {
                assert!(
                    param_str.contains(must_have),
                    "params should contain '{}': {}",
                    must_have,
                    param_str
                );
            }
            if let Some(must_not_have) = must_not_contain {
                assert!(
                    !param_str.contains(must_not_have),
                    "params should NOT contain '{}': {}",
                    must_not_have,
                    param_str
                );
            }
            if let Some(check) = additional_check {
                assert!(
                    param_str.contains(check),
                    "params should contain '{}': {}",
                    check,
                    param_str
                );
            }
        } else {
            assert!(parameters.is_none(), "no params should be sent on success");
        }
    } else {
        panic!("expected ToolCompleted variant");
    }
}

#[test]
fn test_incoming_message_with_timezone() {
    let msg = IncomingMessage::new("test", "user1", "hello").with_timezone("America/New_York");
    assert_eq!(msg.timezone.as_deref(), Some("America/New_York"));
}

/// Minimal channel for blanket-adapter smoke tests.
struct NoopChannel;

impl NativeChannel for NoopChannel {
    fn name(&self) -> &str {
        "noop"
    }
    async fn start(&self) -> Result<MessageStream, ChannelError> {
        use futures::stream;
        Ok(Box::pin(stream::empty()))
    }
    async fn respond(
        &self,
        _msg: &IncomingMessage,
        _response: OutgoingResponse,
    ) -> Result<(), ChannelError> {
        Ok(())
    }
    async fn health_check(&self) -> Result<(), ChannelError> {
        Ok(())
    }
}

/// Verify the `impl<T: NativeChannel> Channel for T` blanket adapter boxes
/// futures correctly and the results cross the `dyn Channel` boundary.
#[tokio::test]
async fn native_channel_blanket_adapter_produces_correct_futures() {
    let ch: Box<dyn Channel> = Box::new(NoopChannel);
    ch.health_check()
        .await
        .expect("health_check should succeed");
    let msg = IncomingMessage::new("noop", "u1", "hi");
    let resp = OutgoingResponse {
        content: "ok".into(),
        thread_id: None,
        attachments: vec![],
        metadata: Default::default(),
    };
    ch.respond(&msg, resp)
        .await
        .expect("respond should succeed");
}

/// Minimal secret-updater for blanket-adapter smoke test.
struct NoopSecretUpdater;

impl NativeChannelSecretUpdater for NoopSecretUpdater {
    async fn update_secret(&self, _new_secret: Option<secrecy::SecretString>) {}
}

/// Verify the `impl<T: NativeChannelSecretUpdater> ChannelSecretUpdater for T`
/// blanket adapter boxes the future correctly.
#[tokio::test]
async fn native_channel_secret_updater_blanket_adapter_boxes_future() {
    let updater = NoopSecretUpdater;
    let dyn_updater: &dyn ChannelSecretUpdater = &updater;
    dyn_updater.update_secret(None).await;
}
