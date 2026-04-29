//! Tests for `validate_strict_schema`, including complex fixture-backed tool schemas.

use super::*;
use crate::tools::tool::{SchemaPath, ToolName};

mod fixture_groups;

#[test]
fn test_valid_schema_passes() {
    let schema = serde_json::json!({
        "type": "object",
        "properties": {
            "name": { "type": "string", "description": "A name" }
        },
        "required": ["name"]
    });
    assert!(validate_strict_schema(&schema, "test").is_ok());
}

#[test]
fn test_strict_schema_accepts_tool_name_newtype() {
    let schema = serde_json::json!({
        "type": "object",
        "properties": {
            "name": { "type": "string" }
        }
    });

    assert!(validate_strict_schema(&schema, ToolName::from("test")).is_ok());
}

fn nested_headers_schema(extra_required: &str) -> serde_json::Value {
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
                    "required": ["name", extra_required]
                }
            }
        }
    })
}

#[test]
fn test_object_schema_accepts_schema_path_newtype() {
    let err = check_object_schema(&nested_headers_schema("missing"), SchemaPath::from("test"));
    assert!(
        err.iter()
            .any(|e| e.contains("test.headers.items") && e.contains("\"missing\""))
    );
}

#[test]
fn test_missing_type_fails() {
    let schema = serde_json::json!({
        "properties": {
            "name": { "type": "string" }
        }
    });
    let err = validate_strict_schema(&schema, "test").unwrap_err();
    assert!(err[0].contains("missing \"type\": \"object\""));
}

#[test]
fn test_wrong_type_fails() {
    let schema = serde_json::json!({ "type": "string" });
    let err = validate_strict_schema(&schema, "test").unwrap_err();
    assert!(err[0].contains("expected type \"object\""));
}

#[test]
fn test_required_not_in_properties_fails() {
    let schema = serde_json::json!({
        "type": "object",
        "properties": {
            "name": { "type": "string" }
        },
        "required": ["name", "age"]
    });
    let err = validate_strict_schema(&schema, "test").unwrap_err();
    assert!(err.iter().any(|e| e.contains("\"age\" not found")));
}

#[test]
fn test_nested_object_validated() {
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
    let err = validate_strict_schema(&schema, "test").unwrap_err();
    assert!(
        err.iter()
            .any(|e| e.contains("test.config") && e.contains("\"missing\""))
    );
}

#[test]
fn test_array_missing_items_fails() {
    let schema = serde_json::json!({
        "type": "object",
        "properties": {
            "tags": { "type": "array", "description": "Tags" }
        }
    });
    let err = validate_strict_schema(&schema, "test").unwrap_err();
    assert!(
        err.iter()
            .any(|e| e.contains("array property missing \"items\""))
    );
}

#[test]
fn test_array_with_items_passes() {
    let schema = serde_json::json!({
        "type": "object",
        "properties": {
            "tags": {
                "type": "array",
                "items": { "type": "string" }
            }
        }
    });
    assert!(validate_strict_schema(&schema, "test").is_ok());
}

#[test]
fn test_forbidden_top_level_keywords_fail() {
    for keyword in ["anyOf", "allOf", "enum", "not"] {
        let mut schema = serde_json::json!({
            "type": "object"
        });
        let root = schema
            .as_object_mut()
            .expect("top-level schema should be an object");

        match keyword {
            "enum" => {
                root.insert(keyword.to_string(), serde_json::json!(["get_repo"]));
            }
            "not" => {
                root.insert(keyword.to_string(), serde_json::json!({ "type": "null" }));
            }
            _ => {
                root.insert(
                    keyword.to_string(),
                    serde_json::json!([
                        {
                            "properties": {
                                "action": { "const": "get_repo" }
                            },
                            "required": ["action"]
                        }
                    ]),
                );
            }
        };

        let err = validate_strict_schema(&schema, "test").unwrap_err();
        assert!(
            err.iter().any(|message| {
                message.contains(&format!("top-level \"{keyword}\" is not allowed"))
            }),
            "expected top-level {keyword} failure, got: {err:?}"
        );
    }
}

#[test]
fn test_top_level_one_of_is_allowed() {
    let schema = serde_json::json!({
        "type": "object",
        "properties": {
            "name": { "type": "string" },
            "url": { "type": "string" },
            "content": { "type": "string" }
        },
        "oneOf": [
            { "required": ["name"] },
            { "required": ["url"] },
            { "required": ["content"] }
        ]
    });

    assert!(
        validate_strict_schema(&schema, "test").is_ok(),
        "root oneOf supports exact-one-source tool contracts"
    );
}

#[test]
fn test_nested_one_of_is_allowed() {
    let schema = serde_json::json!({
        "type": "object",
        "properties": {
            "action": { "type": "string" },
            "inputs": {
                "type": "object",
                "properties": {
                    "mode": {
                        "oneOf": [
                            { "type": "string" },
                            { "type": "integer" }
                        ]
                    }
                },
                "required": ["mode"],
                "additionalProperties": false
            }
        },
        "required": ["action"]
    });

    assert!(
        validate_strict_schema(&schema, "test").is_ok(),
        "nested combinators should not be rejected by root-only validation"
    );
}

#[test]
fn test_enum_type_mismatch_fails() {
    let schema = serde_json::json!({
        "type": "object",
        "properties": {
            "mode": {
                "type": "string",
                "enum": ["fast", 42, "slow"]
            }
        }
    });
    let err = validate_strict_schema(&schema, "test").unwrap_err();
    assert!(err.iter().any(|e| e.contains("enum[1]")));
}

#[test]
fn test_enum_matching_type_passes() {
    let schema = serde_json::json!({
        "type": "object",
        "properties": {
            "mode": {
                "type": "string",
                "enum": ["fast", "slow"]
            }
        }
    });
    assert!(validate_strict_schema(&schema, "test").is_ok());
}

#[test]
fn test_nested_array_items_object_validated() {
    let err = validate_strict_schema(&nested_headers_schema("ghost"), "test").unwrap_err();
    assert!(
        err.iter()
            .any(|e| e.contains("headers.items") && e.contains("\"ghost\""))
    );
}

#[test]
fn test_additional_properties_false_passes() {
    let schema = serde_json::json!({
        "type": "object",
        "properties": {
            "header": {
                "type": "object",
                "properties": {
                    "name": { "type": "string" }
                },
                "additionalProperties": false
            }
        }
    });
    assert!(validate_strict_schema(&schema, "test").is_ok());
}

#[test]
fn test_additional_properties_type_schema_passes() {
    let schema = serde_json::json!({
        "type": "object",
        "properties": {
            "credentials": {
                "type": "object",
                "description": "Map of secret names to env var names",
                "additionalProperties": { "type": "string" }
            }
        }
    });
    assert!(validate_strict_schema(&schema, "test").is_ok());
}
