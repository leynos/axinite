use std::time::Duration;

use insta::assert_snapshot;
use rstest::rstest;

use super::*;
use crate::context::JobContext;
use crate::testing::credentials::TEST_REDACT_SECRET;

fn assert_schema_ok(schema: serde_json::Value) {
    let errors = validate_tool_schema(&schema, "test");
    assert!(errors.is_empty(), "unexpected schema errors: {errors:?}");
}

fn assert_schema_err_contains(schema: serde_json::Value, needle: &str) {
    let errors = validate_tool_schema(&schema, "test");
    assert_eq!(
        errors.len(),
        1,
        "expected exactly one schema error containing '{needle}', got: {errors:?}",
    );
    assert!(
        errors[0].contains(needle),
        "expected schema error containing '{needle}', got: {:?}",
        errors[0],
    );
}

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
fn test_require_str_present() {
    let params = serde_json::json!({"name": "alice"});
    assert_eq!(require_str(&params, "name").unwrap(), "alice");
}

#[test]
fn test_require_str_accepts_param_name() {
    let params = serde_json::json!({"name": "alice"});
    assert_eq!(
        require_str(&params, ParamName::from("name"))
            .expect("expected 'name' parameter to be a string and present"),
        "alice"
    );
}

#[test]
fn test_param_name_preserves_display_value() {
    let name = ParamName::from("name");
    assert_eq!(name.as_ref(), "name");
    assert_eq!(name.to_string(), "name");
}

#[test]
fn test_require_str_missing() {
    let params = serde_json::json!({});
    let err = require_str(&params, "name").unwrap_err();
    assert_eq!(
        err.to_string(),
        "Invalid parameters: missing 'name' parameter"
    );
}

#[test]
fn test_require_str_wrong_type() {
    let params = serde_json::json!({"name": 42});
    let err = require_str(&params, "name").unwrap_err();
    assert!(err.to_string().contains("missing 'name'"));
}

#[test]
fn test_require_param_present() {
    let params = serde_json::json!({"data": [1, 2, 3]});
    assert_eq!(
        require_param(&params, "data").unwrap(),
        &serde_json::json!([1, 2, 3])
    );
}

#[test]
fn test_require_param_missing() {
    let params = serde_json::json!({});
    let err = require_param(&params, "data").unwrap_err();
    assert_eq!(
        err.to_string(),
        "Invalid parameters: missing 'data' parameter"
    );
}

#[test]
fn test_require_param_accepts_param_name_with_unchanged_error() {
    let params = serde_json::json!({});
    let err = require_param(&params, ParamName::from("data")).unwrap_err();
    assert_eq!(
        err.to_string(),
        "Invalid parameters: missing 'data' parameter"
    );
}

#[test]
fn test_schema_path_preserves_display_value() {
    let path = SchemaPath::from("test.headers.items");
    assert_eq!(path.as_ref(), "test.headers.items");
    assert_eq!(path.to_string(), "test.headers.items");
}

#[test]
fn test_schema_path_child_preserves_dot_path_format() {
    let path = SchemaPath::from("test.headers").child("items");
    assert_eq!(path.as_ref(), "test.headers.items");
    assert_eq!(path.to_string(), "test.headers.items");
}

#[test]
fn test_tool_name_preserves_display_value() {
    let tool_name = ToolName::from("github");
    assert_eq!(tool_name.as_ref(), "github");
    assert_eq!(tool_name.to_string(), "github");
}

#[test]
fn test_param_name_from_string_ref_preserves_value() {
    let s = String::from("body");
    let name = ParamName::from(&s);
    assert_eq!(name.as_ref(), "body");
    assert_eq!(name.to_string(), "body");
}

#[test]
fn test_schema_path_from_string_ref_preserves_value() {
    let s = String::from("tool.params");
    let path = SchemaPath::from(&s);
    assert_eq!(path.as_ref(), "tool.params");
    assert_eq!(path.to_string(), "tool.params");
}

#[test]
fn test_tool_name_from_string_ref_preserves_value() {
    let s = String::from("my_tool");
    let name = ToolName::from(&s);
    assert_eq!(name.as_ref(), "my_tool");
    assert_eq!(name.to_string(), "my_tool");
}

#[test]
fn test_tool_name_converts_to_schema_path() {
    let tool = ToolName::from("converter");
    let path = SchemaPath::from(tool);
    assert_eq!(path.as_ref(), "converter");
    assert_eq!(path.to_string(), "converter");
}

#[test]
fn test_validate_tool_schema_nested_path_uses_child() {
    // validate_tool_schema calls SchemaPath::child() when it descends into
    // nested objects; verify the resulting error path is correctly formed.
    let schema = serde_json::json!({
        "type": "object",
        "properties": {
            "config": {
                "type": "object",
                "properties": {
                    "key": { "type": "string" }
                },
                "required": ["key", "absent"]
            }
        }
    });
    let errors = validate_tool_schema(&schema, "root");
    assert!(
        errors
            .iter()
            .any(|e| e.contains("root.config") && e.contains("\"absent\"")),
        "child path must be root.config, got: {errors:?}"
    );
}

#[test]
fn test_requires_approval_default() {
    let tool = EchoTool;
    assert_eq!(
        NativeTool::requires_approval(&tool, &serde_json::json!({"message": "hi"})),
        ApprovalRequirement::Never
    );
    assert_eq!(
        NativeTool::hosted_tool_eligibility(&tool),
        HostedToolEligibility::Eligible
    );
    assert!(!ApprovalRequirement::Never.is_required());
    assert!(ApprovalRequirement::UnlessAutoApproved.is_required());
    assert!(ApprovalRequirement::Always.is_required());
}

#[test]
fn test_redact_params_replaces_sensitive_key() {
    let params = serde_json::json!({"name": "openai_key", "value": TEST_REDACT_SECRET});
    let redacted = redact_params(&params, &["value"]);
    assert_eq!(redacted["name"], "openai_key");
    assert_eq!(redacted["value"], "[REDACTED]");
    assert_eq!(params["value"], TEST_REDACT_SECRET);
}

#[test]
fn test_redact_params_empty_sensitive_is_noop() {
    let params = serde_json::json!({"name": "key", "value": "secret"});
    let redacted = redact_params(&params, &[]);
    assert_eq!(redacted, params);
}

#[test]
fn test_redact_params_missing_key_is_noop() {
    let params = serde_json::json!({"name": "key"});
    let redacted = redact_params(&params, &["value"]);
    assert_eq!(redacted, params);
}

#[test]
fn test_redact_params_non_object_is_passthrough() {
    let params = serde_json::json!("just a string");
    let redacted = redact_params(&params, &["value"]);
    assert_eq!(redacted, params);
}

#[rstest]
#[case(serde_json::json!({
    "type": "object",
    "properties": {
        "name": { "type": "string", "description": "A name" }
    },
    "required": ["name"]
}))]
#[case(serde_json::json!({
    "type": "object",
    "properties": {
        "tags": {
            "type": "array",
            "items": { "type": "string" }
        }
    }
}))]
#[case(serde_json::json!({
    "type": "object",
    "properties": {
        "data": { "description": "Any JSON value" }
    },
    "required": ["data"]
}))]
#[case(serde_json::json!({
    "type": "object",
    "properties": {
        "headers": {
            "type": "array",
            "items": {
                "type": "object",
                "properties": {
                    "name": { "type": "string" },
                    "value": { "type": "string" }
                },
                "required": ["name", "value"]
            }
        }
    }
}))]
fn test_validate_schema_success_cases(#[case] schema: serde_json::Value) {
    assert_schema_ok(schema);
}

#[rstest]
#[case(
    serde_json::json!({
        "properties": {
            "name": { "type": "string" }
        }
    }),
    "missing \"type\": \"object\""
)]
#[case(
    serde_json::json!({
        "type": "string"
    }),
    "expected type \"object\""
)]
#[case(
    serde_json::json!({
        "type": "object",
        "properties": {
            "name": { "type": "string" }
        },
        "required": ["name", "age"]
    }),
    "\"age\" not found in properties"
)]
#[case(
    serde_json::json!({
        "type": "object",
        "properties": {
            "config": {
                "type": "object",
                "properties": {
                    "key": { "type": "string" }
                },
                "required": ["key", "missing"]
            }
        }
    }),
    "test.config: required key \"missing\" not found in properties"
)]
#[case(
    serde_json::json!({
        "type": "object",
        "properties": {
            "tags": { "type": "array", "description": "Tags" }
        }
    }),
    "array property missing \"items\""
)]
#[case(
    serde_json::json!({
        "type": "object",
        "properties": {
            "headers": {
                "type": "array",
                "items": {
                    "type": "object",
                    "properties": {
                        "name": { "type": "string" }
                    },
                    "required": ["name", "missing_field"]
                }
            }
        }
    }),
    "test.headers.items: required key \"missing_field\" not found in properties"
)]
fn test_validate_schema_error_cases(
    #[case] schema: serde_json::Value,
    #[case] expected_fragment: &str,
) {
    assert_schema_err_contains(schema, expected_fragment);
}

#[test]
fn test_approval_context_autonomous_allows_unless_auto_approved() {
    let ctx = ApprovalContext::autonomous();
    assert!(!ctx.is_blocked("shell", ApprovalRequirement::Never));
    assert!(!ctx.is_blocked("shell", ApprovalRequirement::UnlessAutoApproved));
    assert!(ctx.is_blocked("shell", ApprovalRequirement::Always));
}

#[test]
fn test_approval_context_autonomous_with_tools_allows_always() {
    let ctx = ApprovalContext::autonomous_with_tools(["shell".to_string(), "message".to_string()]);
    assert!(!ctx.is_blocked("shell", ApprovalRequirement::Always));
    assert!(!ctx.is_blocked("message", ApprovalRequirement::Always));
    assert!(ctx.is_blocked("http", ApprovalRequirement::Always));
}

#[test]
fn test_approval_context_never_is_not_blocked() {
    let ctx = ApprovalContext::autonomous();
    assert!(!ctx.is_blocked("any_tool", ApprovalRequirement::Never));
}

#[test]
fn test_is_blocked_or_default_with_none_uses_legacy() {
    assert!(!ApprovalContext::is_blocked_or_default(
        &None,
        "any",
        ApprovalRequirement::Never
    ));
    assert!(ApprovalContext::is_blocked_or_default(
        &None,
        "any",
        ApprovalRequirement::UnlessAutoApproved
    ));
    assert!(ApprovalContext::is_blocked_or_default(
        &None,
        "any",
        ApprovalRequirement::Always
    ));
}

#[test]
fn test_is_blocked_or_default_with_some_delegates() {
    let ctx = Some(ApprovalContext::autonomous_with_tools(
        ["shell".to_string()],
    ));
    assert!(!ApprovalContext::is_blocked_or_default(
        &ctx,
        "shell",
        ApprovalRequirement::Always
    ));
    assert!(ApprovalContext::is_blocked_or_default(
        &ctx,
        "other",
        ApprovalRequirement::Always
    ));
    assert!(!ApprovalContext::is_blocked_or_default(
        &ctx,
        "any",
        ApprovalRequirement::UnlessAutoApproved
    ));
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
