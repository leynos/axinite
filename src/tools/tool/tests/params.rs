//! Tests for parameter extraction helpers, name newtypes, and redaction.

use super::super::*;
use crate::testing::credentials::TEST_REDACT_SECRET;

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
fn test_require_str_param_name_error_contains_key() {
    let params = serde_json::json!({});
    let err = require_str(&params, ParamName::from("token")).unwrap_err();
    assert_eq!(
        err.to_string(),
        "Invalid parameters: missing 'token' parameter",
        "ParamName must feed into the error message verbatim"
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
