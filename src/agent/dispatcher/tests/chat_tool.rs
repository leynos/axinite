//! Chat tool execution tests.

use super::super::execute_chat_tool_standalone;
use super::*;

#[tokio::test]
async fn test_execute_chat_tool_standalone_success() {
    use crate::config::SafetyConfig;
    use crate::context::JobContext;
    use crate::safety::SafetyLayer;
    use crate::tools::ToolRegistry;
    use crate::tools::builtin::EchoTool;

    let registry = ToolRegistry::new();
    registry.register_sync(std::sync::Arc::new(EchoTool));

    let safety = SafetyLayer::new(&SafetyConfig {
        max_output_length: 100_000,
        injection_check_enabled: false,
    });

    let job_ctx = JobContext::with_user("test", "chat", "test session");

    let result = execute_chat_tool_standalone(
        &registry,
        &safety,
        &ChatToolRequest {
            tool_name: "echo",
            params: &serde_json::json!({"message": "hello"}),
        },
        &job_ctx,
    )
    .await;

    assert!(result.is_ok());
    let output = result.expect("echo tool execution unexpectedly errored");
    assert!(output.contains("hello"));
}

#[tokio::test]
async fn test_execute_chat_tool_standalone_not_found() {
    use crate::config::SafetyConfig;
    use crate::context::JobContext;
    use crate::safety::SafetyLayer;
    use crate::tools::ToolRegistry;

    let registry = ToolRegistry::new();
    let safety = SafetyLayer::new(&SafetyConfig {
        max_output_length: 100_000,
        injection_check_enabled: false,
    });
    let job_ctx = JobContext::with_user("test", "chat", "test session");

    let result = execute_chat_tool_standalone(
        &registry,
        &safety,
        &ChatToolRequest {
            tool_name: "nonexistent",
            params: &serde_json::json!({}),
        },
        &job_ctx,
    )
    .await;

    assert!(result.is_err());
}
