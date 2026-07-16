//! Unit tests for tool schema validation and parameter helpers.

use std::time::Duration;

use insta::assert_snapshot;

use super::*;
use crate::context::JobContext;

mod approval;
mod params;
mod property_tests;
mod schema;

/// A simple no-op tool for testing.
#[derive(Debug)]
pub struct EchoTool;

impl NativeTool for EchoTool {
    fn name(&self) -> &str {
        "echo"
    }

    fn description(&self) -> &str {
        "Echoes back the input message. Useful for testing."
    }

    fn parameters_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "message": {
                    "type": "string",
                    "description": "The message to echo back"
                }
            },
            "required": ["message"]
        })
    }

    async fn execute(
        &self,
        params: serde_json::Value,
        _ctx: &JobContext,
    ) -> Result<ToolOutput, ToolError> {
        let message = require_str(&params, "message")?;

        Ok(ToolOutput::text(message, Duration::from_millis(1)))
    }

    fn requires_sanitization(&self) -> bool {
        false
    }
}

#[tokio::test]
async fn test_echo_tool() {
    let tool = EchoTool;
    let ctx = JobContext::default();

    let result = NativeTool::execute(&tool, serde_json::json!({"message": "hello"}), &ctx)
        .await
        .unwrap();

    assert_eq!(result.result, serde_json::json!("hello"));
}

#[test]
fn test_tool_schema() {
    let tool = EchoTool;
    let schema = NativeTool::schema(&tool);

    assert_eq!(schema.name, "echo");
    assert!(!schema.description.is_empty());
}

#[test]
fn test_execution_timeout_default() {
    let tool = EchoTool;
    assert_eq!(
        NativeTool::execution_timeout(&tool),
        Duration::from_secs(60)
    );
}

#[test]
fn test_require_str_missing_error_snapshot() {
    let params = serde_json::json!({});
    let err = require_str(&params, "token").unwrap_err().to_string();
    assert_snapshot!(err);
}

#[test]
fn test_require_param_missing_error_snapshot() {
    let params = serde_json::json!({});
    let err = require_param(&params, "data").unwrap_err().to_string();
    assert_snapshot!(err);
}
