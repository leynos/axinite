use rstest::rstest;
use serde_json::Value as JsonValue;

use crate::llm::schema_normalize::normalize_schema_strict;
use crate::llm::test_fixtures::github_style_schema;

#[rstest]
fn test_normalize_schema_strict_flattens_top_level_oneof(github_style_schema: JsonValue) {
    let normalized = normalize_schema_strict(&github_style_schema);

    assert_eq!(normalized["type"], serde_json::json!("object"));
    assert!(
        normalized.get("oneOf").is_none(),
        "top-level oneOf must be removed for OpenAI compatibility: {normalized}"
    );
    assert!(
        normalized.get("anyOf").is_none(),
        "top-level anyOf must be removed for OpenAI compatibility: {normalized}"
    );
    assert_eq!(
        normalized["properties"]["action"]["enum"],
        serde_json::json!(["create_issue", "get_repo"])
    );
    assert_eq!(
        normalized["properties"]["owner"]["type"],
        serde_json::json!("string")
    );
    assert_eq!(
        normalized["properties"]["title"]["type"],
        serde_json::json!(["string", "null"])
    );
}

#[test]
fn test_normalize_schema_strict_preserves_typed_map_objects() {
    let normalized = normalize_schema_strict(&serde_json::json!({
        "type": "object",
        "properties": {
            "action": { "type": "string" },
            "inputs": {
                "type": "object",
                "additionalProperties": { "type": "string" }
            }
        },
        "required": ["action"]
    }));

    assert_eq!(
        normalized["properties"]["inputs"]["additionalProperties"],
        serde_json::json!({ "type": "string" })
    );
    assert!(
        normalized["properties"]["inputs"]
            .get("properties")
            .is_none(),
        "typed map objects should not be rewritten into empty fixed-shape objects: {normalized}"
    );
}

#[test]
fn test_normalize_schema_strict_treats_empty_properties_as_fixed_shape_object() {
    let normalized = normalize_schema_strict(&serde_json::json!({
        "type": "object",
        "properties": {
            "inputs": {
                "type": "object",
                "properties": {},
                "additionalProperties": { "type": "string" }
            }
        }
    }));

    assert_eq!(
        normalized["properties"]["inputs"]["additionalProperties"],
        serde_json::json!(false)
    );
    assert_eq!(
        normalized["properties"]["inputs"]["properties"],
        serde_json::json!({})
    );
}

#[test]
fn test_normalize_schema_strict_merges_shared_nested_object_properties() {
    let normalized = normalize_schema_strict(&serde_json::json!({
        "type": "object",
        "required": ["action"],
        "oneOf": [
            {
                "properties": {
                    "action": { "const": "first" },
                    "inputs": {
                        "type": "object",
                        "properties": {
                            "owner": { "type": "string" }
                        },
                        "required": ["owner"],
                        "additionalProperties": false
                    }
                },
                "required": ["action", "inputs"]
            },
            {
                "properties": {
                    "action": { "const": "second" },
                    "inputs": {
                        "type": "object",
                        "properties": {
                            "repo": { "type": "string" }
                        },
                        "required": ["repo"],
                        "additionalProperties": false
                    }
                },
                "required": ["action", "inputs"]
            }
        ]
    }));

    assert!(
        normalized["properties"]["inputs"]["properties"]["owner"].is_object(),
        "expected first nested property to survive merge: {normalized}"
    );
    assert!(
        normalized["properties"]["inputs"]["properties"]["repo"].is_object(),
        "expected later nested property to survive merge: {normalized}"
    );
}
