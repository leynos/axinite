//! Unit tests for tool execution with safety checks applied.

use super::*;
use crate::tools::tool::{NativeTool, Tool, ToolError, ToolOutput};
use std::sync::Arc;
use std::time::Duration;

struct EchoTool;

impl NativeTool for EchoTool {
    fn name(&self) -> &str {
        "echo"
    }
    fn description(&self) -> &str {
        "Echoes input"
    }
    fn parameters_schema(&self) -> serde_json::Value {
        serde_json::json!({"type": "object", "properties": {}})
    }
    async fn execute(
        &self,
        params: serde_json::Value,
        _ctx: &JobContext,
    ) -> Result<ToolOutput, ToolError> {
        Ok(ToolOutput::success(params, Duration::default()))
    }
    fn requires_sanitization(&self) -> bool {
        false
    }
}

struct FailTool;

impl NativeTool for FailTool {
    fn name(&self) -> &str {
        "fail_tool"
    }
    fn description(&self) -> &str {
        "Always fails"
    }
    fn parameters_schema(&self) -> serde_json::Value {
        serde_json::json!({"type": "object", "properties": {}})
    }
    async fn execute(&self, _: serde_json::Value, _: &JobContext) -> Result<ToolOutput, ToolError> {
        Err(ToolError::ExecutionFailed(
            "intentional failure".to_string(),
        ))
    }
    fn requires_sanitization(&self) -> bool {
        false
    }
}

struct SlowTool;

impl NativeTool for SlowTool {
    fn name(&self) -> &str {
        "slow_tool"
    }
    fn description(&self) -> &str {
        "Sleeps forever"
    }
    fn parameters_schema(&self) -> serde_json::Value {
        serde_json::json!({"type": "object", "properties": {}})
    }
    async fn execute(&self, _: serde_json::Value, _: &JobContext) -> Result<ToolOutput, ToolError> {
        tokio::time::sleep(Duration::from_secs(60)).await;
        unreachable!()
    }
    fn execution_timeout(&self) -> Duration {
        Duration::from_millis(50)
    }
    fn requires_sanitization(&self) -> bool {
        false
    }
}

fn test_safety() -> SafetyLayer {
    SafetyLayer::new(&crate::config::SafetyConfig {
        max_output_length: 100_000,
        injection_check_enabled: false,
    })
}

fn test_job_ctx() -> JobContext {
    JobContext::default()
}

enum RegistrationMode {
    Dynamic,
    Privileged,
}

async fn registry_with(tools: Vec<(Arc<dyn Tool>, RegistrationMode)>) -> ToolRegistry {
    let registry = ToolRegistry::new();
    for (tool, mode) in tools {
        match mode {
            RegistrationMode::Dynamic => {
                registry.register(tool).await;
            }
            RegistrationMode::Privileged => {
                registry.register_sync(Arc::clone(&tool));
            }
        }
    }
    registry
}

#[tokio::test]
async fn test_execute_success() {
    let registry = registry_with(vec![(Arc::new(EchoTool), RegistrationMode::Privileged)]).await;
    let safety = test_safety();
    let params = serde_json::json!({"message": "hello"});

    let result =
        execute_tool_with_safety(&registry, &safety, "echo", &params, &test_job_ctx()).await;

    assert!(result.is_ok(), "Echo tool should succeed");
    let output = result.unwrap();
    assert!(
        output.contains("hello"),
        "Output should contain the echoed input"
    );
}

#[tokio::test]
async fn test_execute_missing_tool() {
    let registry = registry_with(vec![]).await;
    let safety = test_safety();

    let result = execute_tool_with_safety(
        &registry,
        &safety,
        "nonexistent",
        &serde_json::json!({}),
        &test_job_ctx(),
    )
    .await;

    assert!(result.is_err(), "Missing tool should return error");
    let err = result.unwrap_err().to_string();
    assert!(
        err.contains("nonexistent") || err.contains("not found"),
        "Error should mention the tool: {}",
        err
    );
}

#[tokio::test]
async fn test_execute_tool_failure() {
    let registry = registry_with(vec![(Arc::new(FailTool), RegistrationMode::Dynamic)]).await;
    let safety = test_safety();

    let result = execute_tool_with_safety(
        &registry,
        &safety,
        "fail_tool",
        &serde_json::json!({}),
        &test_job_ctx(),
    )
    .await;

    assert!(result.is_err(), "FailTool should return error");
    let err = result.unwrap_err().to_string();
    assert!(
        err.contains("intentional failure"),
        "Error should contain the failure reason: {}",
        err
    );
}

#[tokio::test]
async fn test_execute_tool_timeout() {
    let registry = registry_with(vec![(Arc::new(SlowTool), RegistrationMode::Dynamic)]).await;
    let safety = test_safety();

    let start = std::time::Instant::now();
    let result = execute_tool_with_safety(
        &registry,
        &safety,
        "slow_tool",
        &serde_json::json!({}),
        &test_job_ctx(),
    )
    .await;
    let elapsed = start.elapsed();

    assert!(result.is_err(), "SlowTool should timeout");
    let err = result.unwrap_err().to_string();
    assert!(
        err.to_lowercase().contains("timeout") || err.to_lowercase().contains("timed out"),
        "Error should mention timeout: {}",
        err
    );
    assert!(
        elapsed < Duration::from_secs(1),
        "Should timeout quickly, not wait 60s"
    );
}

#[test]
fn test_process_tool_result_success() {
    let safety = test_safety();
    let result: Result<String, String> = Ok("tool output data".to_string());

    let (content, message) = process_tool_result(&safety, "echo", "call_1", &result);

    assert!(
        content.contains("tool_output"),
        "Content should be XML-wrapped: {}",
        content
    );
    assert!(
        content.contains("tool output data"),
        "Content should contain the output: {}",
        content
    );
    assert_eq!(message.role, crate::llm::Role::Tool);
    assert_eq!(message.name.as_deref(), Some("echo"));
}

#[test]
fn test_process_tool_result_error() {
    let safety = test_safety();
    let result: Result<String, String> = Err("something went wrong".to_string());

    let (content, message) = process_tool_result(&safety, "echo", "call_1", &result);

    // Error content is now XML-wrapped like success content (routed through safety pipeline)
    assert!(
        content.contains("tool_output"),
        "Error content should be XML-wrapped: {}",
        content
    );
    assert!(
        content.contains("Error:"),
        "Error content should contain 'Error:': {}",
        content
    );
    assert!(
        content.contains("something went wrong"),
        "Error content should contain the message: {}",
        content
    );
    assert_eq!(message.role, crate::llm::Role::Tool);
    assert_eq!(message.name.as_deref(), Some("echo"));
}

/// Test that error content is routed through the safety pipeline,
/// ensuring consistent formatting between live execution and reconstructed history.
#[test]
fn test_process_tool_result_error_uses_safety_pipeline() {
    let safety = test_safety();
    let error_result: Result<String, String> = Err("test error".to_string());
    let success_result: Result<String, String> = Ok("test success".to_string());

    let (error_content, _) = process_tool_result(&safety, "test_tool", "call_1", &error_result);
    let (success_content, _) = process_tool_result(&safety, "test_tool", "call_2", &success_result);

    // Both error and success content should be XML-wrapped by the safety pipeline
    assert!(
        error_content.contains("<tool_output "),
        "Error content should be XML-wrapped with <tool_output>: {}",
        error_content
    );
    assert!(
        success_content.contains("<tool_output "),
        "Success content should be XML-wrapped with <tool_output>: {}",
        success_content
    );

    // Both should end with </tool_output>
    assert!(
        error_content.contains("</tool_output>"),
        "Error content should close XML tag: {}",
        error_content
    );
    assert!(
        success_content.contains("</tool_output>"),
        "Success content should close XML tag: {}",
        success_content
    );
}
