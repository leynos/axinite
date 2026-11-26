//! Unit tests for the supported schema subset and argument validation.

use super::*;
use pretty_assertions::assert_eq;
use serde_json::json;

fn echo_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "message": {"type": "string", "description": "text"}
        },
        "required": ["message"]
    })
}

#[test]
fn accepts_supported_schema() {
    validate_schema(&echo_schema()).expect("schema");
    validate_arguments(&echo_schema(), &json!({"message": "hi"})).expect("arguments");
}

#[test]
fn rejects_missing_required() {
    let error = validate_arguments(&echo_schema(), &json!({})).expect_err("required");
    assert_eq!(error.field, "arguments.message");
}

#[test]
fn rejects_unknown_argument() {
    let error = validate_arguments(&echo_schema(), &json!({"message": "x", "other": 1}))
        .expect_err("unknown");
    assert_eq!(error.field, "arguments.other");
}

#[test]
fn rejects_explicit_null_for_optional_string() {
    let schema = json!({
        "type": "object",
        "properties": {"message": {"type": "string"}}
    });
    assert!(validate_arguments(&schema, &json!({"message": null})).is_err());
}

#[test]
fn accepts_nullable_string() {
    let schema = json!({
        "type": "object",
        "properties": {"message": {"type": ["string", "null"]}}
    });
    validate_arguments(&schema, &json!({"message": null})).expect("nullable");
}

#[test]
fn accepts_omitted_optional() {
    let schema = json!({
        "type": "object",
        "properties": {"message": {"type": "string"}}
    });
    validate_arguments(&schema, &json!({})).expect("omitted");
}

#[test]
fn rejects_unsupported_keyword() {
    let schema = json!({
        "type": "object",
        "properties": {"message": {"type": "string", "pattern": "^a"}}
    });
    let error = validate_schema(&schema).expect_err("pattern");
    assert!(error.message.contains("unsupported"));
}

#[test]
fn rejects_additional_properties_true() {
    let schema = json!({
        "type": "object",
        "properties": {},
        "additionalProperties": true
    });
    assert!(validate_schema(&schema).is_err());
}

#[test]
fn enforces_enum() {
    let schema = json!({
        "type": "object",
        "properties": {"mode": {"type": "string", "enum": ["a", "b"]}}
    });
    validate_arguments(&schema, &json!({"mode": "a"})).expect("enum");
    assert!(validate_arguments(&schema, &json!({"mode": "c"})).is_err());
}

#[test]
fn validates_nested_objects_and_arrays() {
    let schema = json!({
        "type": "object",
        "properties": {
            "rows": {
                "type": "array",
                "items": {
                    "type": "object",
                    "properties": {"id": {"type": "integer"}},
                    "required": ["id"]
                }
            }
        }
    });
    validate_arguments(&schema, &json!({"rows": [{"id": 1}]})).expect("nested");
    assert!(validate_arguments(&schema, &json!({"rows": [{"id": "x"}]})).is_err());
    assert!(validate_arguments(&schema, &json!({"rows": [{}]})).is_err());
}

#[test]
fn freeform_property_accepts_any_bounded_json() {
    let schema = json!({
        "type": "object",
        "properties": {"data": {"description": "freeform"}}
    });
    validate_arguments(&schema, &json!({"data": {"any": [1, null]}})).expect("freeform");
}
