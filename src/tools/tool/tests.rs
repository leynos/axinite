use std::time::Duration;

use async_trait::async_trait;

use super::*;
use crate::context::JobContext;
use crate::testing::credentials::TEST_REDACT_SECRET;

/// A simple no-op tool for testing.
#[derive(Debug)]
pub struct EchoTool;

#[async_trait]
impl Tool for EchoTool {
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

    let result = tool
        .execute(serde_json::json!({"message": "hello"}), &ctx)
        .await
        .unwrap();

    assert_eq!(result.result, serde_json::json!("hello"));
}

#[test]
fn test_tool_schema() {
    let tool = EchoTool;
    let schema = tool.schema();

    assert_eq!(schema.name, "echo");
    assert!(!schema.description.is_empty());
}

#[test]
fn test_execution_timeout_default() {
    let tool = EchoTool;
    assert_eq!(tool.execution_timeout(), Duration::from_secs(60));
}

#[test]
fn test_require_str_present() {
    let params = serde_json::json!({"name": "alice"});
    assert_eq!(require_str(&params, "name").unwrap(), "alice");
}

#[test]
fn test_require_str_missing() {
    let params = serde_json::json!({});
    let err = require_str(&params, "name").unwrap_err();
    assert!(err.to_string().contains("missing 'name'"));
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
    assert!(err.to_string().contains("missing 'data'"));
}

#[test]
fn test_requires_approval_default() {
    let tool = EchoTool;
    assert_eq!(
        tool.requires_approval(&serde_json::json!({"message": "hi"})),
        ApprovalRequirement::Never
    );
    assert_eq!(
        tool.hosted_tool_eligibility(),
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

#[test]
fn test_validate_schema_valid() {
    let schema = serde_json::json!({
        "type": "object",
        "properties": {
            "name": { "type": "string", "description": "A name" }
        },
        "required": ["name"]
    });
    let errors = validate_tool_schema(&schema, "test");
    assert!(errors.is_empty(), "unexpected errors: {errors:?}");
}

#[test]
fn test_validate_schema_missing_type() {
    let schema = serde_json::json!({
        "properties": {
            "name": { "type": "string" }
        }
    });
    let errors = validate_tool_schema(&schema, "test");
    assert_eq!(errors.len(), 1);
    assert!(errors[0].contains("missing \"type\": \"object\""));
}

#[test]
fn test_validate_schema_wrong_type() {
    let schema = serde_json::json!({
        "type": "string"
    });
    let errors = validate_tool_schema(&schema, "test");
    assert_eq!(errors.len(), 1);
    assert!(errors[0].contains("expected type \"object\""));
}

#[test]
fn test_validate_schema_required_not_in_properties() {
    let schema = serde_json::json!({
        "type": "object",
        "properties": {
            "name": { "type": "string" }
        },
        "required": ["name", "age"]
    });
    let errors = validate_tool_schema(&schema, "test");
    assert_eq!(errors.len(), 1);
    assert!(errors[0].contains("\"age\" not found in properties"));
}

#[test]
fn test_validate_schema_nested_object() {
    let schema = serde_json::json!({
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
    });
    let errors = validate_tool_schema(&schema, "test");
    assert_eq!(errors.len(), 1);
    assert!(errors[0].contains("test.config"));
    assert!(errors[0].contains("\"missing\" not found"));
}

#[test]
fn test_validate_schema_array_missing_items() {
    let schema = serde_json::json!({
        "type": "object",
        "properties": {
            "tags": { "type": "array", "description": "Tags" }
        }
    });
    let errors = validate_tool_schema(&schema, "test");
    assert_eq!(errors.len(), 1);
    assert!(errors[0].contains("array property missing \"items\""));
}

#[test]
fn test_validate_schema_array_with_items_ok() {
    let schema = serde_json::json!({
        "type": "object",
        "properties": {
            "tags": {
                "type": "array",
                "items": { "type": "string" }
            }
        }
    });
    let errors = validate_tool_schema(&schema, "test");
    assert!(errors.is_empty(), "unexpected errors: {errors:?}");
}

#[test]
fn test_validate_schema_freeform_property_allowed() {
    let schema = serde_json::json!({
        "type": "object",
        "properties": {
            "data": { "description": "Any JSON value" }
        },
        "required": ["data"]
    });
    let errors = validate_tool_schema(&schema, "test");
    assert!(
        errors.is_empty(),
        "freeform property should be allowed: {errors:?}"
    );
}

#[test]
fn test_validate_schema_nested_array_items_object() {
    let schema = serde_json::json!({
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
    });
    let errors = validate_tool_schema(&schema, "test");
    assert!(errors.is_empty(), "unexpected errors: {errors:?}");
}

#[test]
fn test_validate_schema_nested_array_items_object_bad() {
    let schema = serde_json::json!({
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
    });
    let errors = validate_tool_schema(&schema, "test");
    assert_eq!(errors.len(), 1);
    assert!(errors[0].contains("headers.items"));
    assert!(errors[0].contains("\"missing_field\""));
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
