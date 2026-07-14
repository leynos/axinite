//! Tests for tool JSON schema validation.

use rstest::rstest;

use super::super::*;

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
