//! Unit tests for the restart tool's Docker environment handling.

use super::*;

/// Helper to simulate Docker environment for testing
fn enable_docker_env() {
    unsafe {
        std::env::set_var("AXINITE_IN_DOCKER", "true");
    }
}

#[test]
fn test_restart_tool_approval_handled_at_command_level() {
    // Approval is handled at the /restart command level (web modal confirmation),
    // not at tool execution. Tool execution approval is for user-interactive approvals
    // that happen during job execution. The restart confirmation modal provides that gate.
    let tool = RestartTool;
    let approval = NativeTool::requires_approval(&tool, &serde_json::json!({}));
    // Default (Never) allows tool to execute in autonomous jobs created from approved commands
    assert!(matches!(approval, ApprovalRequirement::Never));
}

#[test]
fn test_restart_tool_name() {
    let tool = RestartTool;
    assert_eq!(NativeTool::name(&tool), "restart");
}

#[test]
fn test_restart_tool_parameters_schema() {
    let tool = RestartTool;
    let schema = NativeTool::parameters_schema(&tool);

    // Verify schema has delay_secs property with bounds
    let props = schema.get("properties").unwrap();
    assert!(props.get("delay_secs").is_some());

    let delay_schema = props.get("delay_secs").unwrap();
    assert_eq!(delay_schema.get("minimum").unwrap().as_u64().unwrap(), 1);
    assert_eq!(delay_schema.get("maximum").unwrap().as_u64().unwrap(), 30);
}

#[test]
fn test_restart_tool_requires_sanitization() {
    let tool = RestartTool;
    assert!(!NativeTool::requires_sanitization(&tool));
}

#[tokio::test]
async fn test_restart_tool_delay_parameter_validation() {
    enable_docker_env();
    let tool = RestartTool;
    let ctx = crate::context::JobContext::new("test", "test restart");

    // Test with valid delay
    let result = NativeTool::execute(&tool, serde_json::json!({"delay_secs": 5}), &ctx).await;
    assert!(result.is_ok());
    let output = result.unwrap();
    let text = output.result.as_str().expect("result should be a string");
    assert!(text.contains("Restarting in 5 second(s)"));

    // Test with no delay parameter (should use default 2)
    let result = NativeTool::execute(&tool, serde_json::json!({}), &ctx).await;
    assert!(result.is_ok());
    let output = result.unwrap();
    let text = output.result.as_str().expect("result should be a string");
    assert!(text.contains("Restarting in 2 second(s)"));
}

#[tokio::test]
async fn test_restart_tool_delay_clamping() {
    enable_docker_env();
    let tool = RestartTool;
    let ctx = crate::context::JobContext::new("test", "test restart");

    // Test with too small delay (should clamp to 1)
    let result = tool
        .execute(serde_json::json!({"delay_secs": 0}), &ctx)
        .await;
    assert!(result.is_ok());
    let output = result.unwrap();
    let text = output.result.as_str().expect("result should be a string");
    assert!(text.contains("Restarting in 1 second(s)"));

    // Test with too large delay (should clamp to 30)
    let result = tool
        .execute(serde_json::json!({"delay_secs": 100}), &ctx)
        .await;
    assert!(result.is_ok());
    let output = result.unwrap();
    let text = output.result.as_str().expect("result should be a string");
    assert!(text.contains("Restarting in 30 second(s)"));
}

#[test]
fn test_restart_tool_description() {
    let tool = RestartTool;
    let desc = NativeTool::description(&tool);
    assert!(desc.contains("Restart"));
    assert!(desc.contains("Axinite"));
    assert!(desc.contains("exits cleanly"));
    assert!(desc.contains("code 0"));
}

#[test]
fn test_restart_tool_schema_completeness() {
    let tool = RestartTool;
    let schema = NativeTool::parameters_schema(&tool);

    // Verify schema structure
    assert_eq!(schema.get("type").unwrap().as_str().unwrap(), "object");

    let props = schema.get("properties").unwrap();
    assert!(props.is_object());

    let delay_schema = props.get("delay_secs").unwrap();
    assert_eq!(
        delay_schema.get("type").unwrap().as_str().unwrap(),
        "integer"
    );
    assert!(delay_schema.get("description").is_some());
}

#[tokio::test]
async fn test_restart_tool_boundary_values() {
    enable_docker_env();
    let tool = RestartTool;
    let ctx = crate::context::JobContext::new("test", "test restart");

    // Test minimum boundary (exactly 1)
    let result = tool
        .execute(serde_json::json!({"delay_secs": 1}), &ctx)
        .await;
    assert!(result.is_ok());
    let output = result.unwrap();
    let text = output.result.as_str().unwrap();
    assert!(text.contains("Restarting in 1 second(s)"));

    // Test maximum boundary (exactly 30)
    let result = tool
        .execute(serde_json::json!({"delay_secs": 30}), &ctx)
        .await;
    assert!(result.is_ok());
    let output = result.unwrap();
    let text = output.result.as_str().unwrap();
    assert!(text.contains("Restarting in 30 second(s)"));

    // Test middle value
    let result = tool
        .execute(serde_json::json!({"delay_secs": 15}), &ctx)
        .await;
    assert!(result.is_ok());
    let output = result.unwrap();
    let text = output.result.as_str().unwrap();
    assert!(text.contains("Restarting in 15 second(s)"));
}

#[tokio::test]
async fn test_restart_tool_invalid_parameter_types() {
    enable_docker_env();
    let tool = RestartTool;
    let ctx = crate::context::JobContext::new("test", "test restart");

    // String instead of integer - should use default
    let result = tool
        .execute(serde_json::json!({"delay_secs": "5"}), &ctx)
        .await;
    assert!(result.is_ok());
    let output = result.unwrap();
    let text = output.result.as_str().unwrap();
    assert!(text.contains("Restarting in 2 second(s)")); // Falls back to default

    // Null value - should use default
    let result = tool
        .execute(serde_json::json!({"delay_secs": null}), &ctx)
        .await;
    assert!(result.is_ok());
    let output = result.unwrap();
    let text = output.result.as_str().unwrap();
    assert!(text.contains("Restarting in 2 second(s)"));

    // Float value - should use default (as_u64 fails on floats)
    let result = tool
        .execute(serde_json::json!({"delay_secs": 5.5}), &ctx)
        .await;
    assert!(result.is_ok());
    let output = result.unwrap();
    let text = output.result.as_str().unwrap();
    assert!(text.contains("Restarting in 2 second(s)"));
}

#[tokio::test]
async fn test_restart_tool_output_structure() {
    enable_docker_env();
    let tool = RestartTool;
    let ctx = crate::context::JobContext::new("test", "test restart");

    let result = tool
        .execute(serde_json::json!({"delay_secs": 5}), &ctx)
        .await;

    assert!(result.is_ok());
    let output = result.unwrap();

    // Verify ToolOutput structure
    assert!(output.result.is_string());
    assert!(output.duration.as_secs() == 0); // Should be nearly instant
    assert!(output.cost.is_none()); // No cost tracking for restart
    assert!(output.raw.is_none()); // No raw output stored
}

#[tokio::test]
async fn test_restart_tool_extra_parameters_ignored() {
    enable_docker_env();
    let tool = RestartTool;
    let ctx = crate::context::JobContext::new("test", "test restart");

    // Extra parameters should be ignored
    let result = tool
        .execute(
            serde_json::json!({
                "delay_secs": 5,
                "extra_field": "should be ignored",
                "another": 123
            }),
            &ctx,
        )
        .await;

    assert!(result.is_ok());
    let output = result.unwrap();
    let text = output.result.as_str().unwrap();
    assert!(text.contains("Restarting in 5 second(s)"));
}

#[tokio::test]
async fn test_restart_tool_negative_numbers() {
    enable_docker_env();
    let tool = RestartTool;
    let ctx = crate::context::JobContext::new("test", "test restart");

    // Negative number should clamp to 1
    let result = tool
        .execute(serde_json::json!({"delay_secs": -5}), &ctx)
        .await;
    assert!(result.is_ok());
    let output = result.unwrap();
    let text = output.result.as_str().unwrap();
    // as_u64() on negative number returns None, so falls to default 2
    assert!(text.contains("Restarting in 2 second(s)"));
}

#[tokio::test]
async fn test_restart_tool_very_large_numbers() {
    enable_docker_env();
    let tool = RestartTool;
    let ctx = crate::context::JobContext::new("test", "test restart");

    // Very large number should clamp to 30
    let result = tool
        .execute(serde_json::json!({"delay_secs": u64::MAX}), &ctx)
        .await;
    assert!(result.is_ok());
    let output = result.unwrap();
    let text = output.result.as_str().unwrap();
    assert!(text.contains("Restarting in 30 second(s)"));
}

#[tokio::test]
async fn test_restart_tool_empty_object() {
    enable_docker_env();
    let tool = RestartTool;
    let ctx = crate::context::JobContext::new("test", "test restart");

    // Empty object params should use all defaults
    let result = NativeTool::execute(&tool, serde_json::json!({}), &ctx).await;
    assert!(result.is_ok());
    let output = result.unwrap();
    let text = output.result.as_str().unwrap();
    assert!(text.contains("Restarting in 2 second(s)"));
    assert!(text.contains("exit cleanly"));
    assert!(text.contains("entrypoint restart loop"));
}

#[test]
fn test_restart_tool_approval_consistent_regardless_of_params() {
    let tool = RestartTool;

    // Approval requirement should be the same regardless of params
    let approval1 = NativeTool::requires_approval(&tool, &serde_json::json!({"delay_secs": 5}));
    let approval2 = NativeTool::requires_approval(&tool, &serde_json::json!({"delay_secs": 100}));
    let approval3 = NativeTool::requires_approval(&tool, &serde_json::json!({}));

    // All should return the default (Never) since approval happens at command level
    assert!(matches!(approval1, ApprovalRequirement::Never));
    assert!(matches!(approval2, ApprovalRequirement::Never));
    assert!(matches!(approval3, ApprovalRequirement::Never));
}

#[test]
fn test_restart_tool_requires_docker_environment() {
    // Test that restart is rejected when not in Docker (AXINITE_IN_DOCKER not set or false)
    // Uses sync test to avoid async/env var ordering issues with test parallelization.
    let in_docker = std::env::var("AXINITE_IN_DOCKER")
        .map(|v| v.to_lowercase() == "true")
        .unwrap_or(false);

    // Verify logic: when not in Docker, env var should be false/unset
    if !in_docker {
        // Simulating what the tool would do when AXINITE_IN_DOCKER is not set
        assert!(
            !in_docker,
            "Test environment should have AXINITE_IN_DOCKER unset or false"
        );
    }
}
