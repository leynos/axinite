//! Tests for channel trait and message types.

use super::*;
use crate::testing::credentials::TEST_REDACT_SECRET_123;

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

#[test]
fn tool_completed_redacts_sensitive_params_on_failure() {
    let params = serde_json::json!({"name": "api_key", "value": TEST_REDACT_SECRET_123});
    let err: Result<String, crate::error::Error> = Err(crate::error::ToolError::ExecutionFailed {
        name: "secret_save".into(),
        reason: "db error".into(),
    }
    .into());
    let tool = SecretTool;

    let status = StatusUpdate::tool_completed(
        "secret_save".into(),
        &err,
        &params,
        Some(&tool as &dyn crate::tools::Tool),
    );

    if let StatusUpdate::ToolCompleted {
        success,
        error,
        parameters,
        ..
    } = &status
    {
        assert!(!success);
        let err_msg = error.as_deref().expect("should have error");
        assert!(err_msg.contains("db error"), "error: {}", err_msg);
        let param_str = parameters
            .as_ref()
            .expect("should have parameters on failure");
        assert!(
            param_str.contains("[REDACTED]"),
            "sensitive value should be redacted: {}",
            param_str
        );
        assert!(
            !param_str.contains(TEST_REDACT_SECRET_123),
            "raw secret should not appear: {}",
            param_str
        );
        assert!(
            param_str.contains("api_key"),
            "non-sensitive params should be preserved: {}",
            param_str
        );
    } else {
        panic!("expected ToolCompleted variant");
    }
}

#[test]
fn tool_completed_no_params_on_success() {
    let params = serde_json::json!({"name": "key", "value": "secret"});
    let ok: Result<String, crate::error::Error> = Ok("done".into());

    let status = StatusUpdate::tool_completed("secret_save".into(), &ok, &params, None);

    if let StatusUpdate::ToolCompleted {
        success,
        error,
        parameters,
        ..
    } = &status
    {
        assert!(success);
        assert!(error.is_none());
        assert!(parameters.is_none(), "no params should be sent on success");
    } else {
        panic!("expected ToolCompleted variant");
    }
}

#[test]
fn tool_completed_no_tool_passes_params_unredacted() {
    let params = serde_json::json!({"cmd": "ls -la"});
    let err: Result<String, crate::error::Error> = Err(crate::error::ToolError::ExecutionFailed {
        name: "shell".into(),
        reason: "timeout".into(),
    }
    .into());

    let status = StatusUpdate::tool_completed("shell".into(), &err, &params, None);

    if let StatusUpdate::ToolCompleted { parameters, .. } = &status {
        let param_str = parameters.as_ref().expect("should have parameters");
        assert!(
            param_str.contains("ls -la"),
            "non-sensitive params should pass through: {}",
            param_str
        );
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
