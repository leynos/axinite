//! Tests for trace-template substitution helpers.

use std::collections::HashMap;

use ironclaw::llm::{ChatMessage, Role};
use rstest::{fixture, rstest};

use crate::support::trace_template_utils::{
    extract_tool_result_vars, has_template_marker, substitute_templates,
};

#[fixture]
fn tool_message_fixture() -> ChatMessage {
    ChatMessage {
        role: Role::Tool,
        content: serde_json::json!({"value": "default"}).to_string(),
        content_parts: Vec::new(),
        tool_call_id: Some("call".to_string()),
        name: None,
        tool_calls: None,
    }
}

#[test]
fn substitute_templates_stops_on_cyclic_references() {
    let vars = HashMap::from([
        ("first".to_string(), serde_json::json!("{{second}}")),
        ("second".to_string(), serde_json::json!("{{first}}")),
    ]);
    let mut value = serde_json::json!("path {{first}}");

    substitute_templates(&mut value, &vars);

    assert_eq!(value, serde_json::json!("path {{first}}"));
}

#[test]
fn substitute_templates_allows_repeated_non_cyclic_keys() {
    let vars = HashMap::from([("name".to_string(), serde_json::json!("Ada"))]);
    let mut value = serde_json::json!("{{name}} meets {{name}}");

    substitute_templates(&mut value, &vars);

    assert_eq!(value, serde_json::json!("Ada meets Ada"));
}

#[test]
fn substitute_templates_preserves_scalar_json_types() {
    let vars = HashMap::from([
        ("limit".to_string(), serde_json::json!(3)),
        ("enabled".to_string(), serde_json::json!(true)),
    ]);
    let mut value = serde_json::json!({
        "limit": "{{limit}}",
        "enabled": "{{enabled}}",
        "summary": "limit={{limit}}, enabled={{enabled}}",
    });

    substitute_templates(&mut value, &vars);

    assert_eq!(
        value,
        serde_json::json!({
            "limit": 3,
            "enabled": true,
            "summary": "limit=3, enabled=true",
        })
    );
}

#[test]
fn substitute_templates_resolves_chained_string_templates() {
    // {{a}} resolves to "{{b}}", which should then resolve to 1.
    let vars = HashMap::from([
        ("a".to_string(), serde_json::json!("{{b}}")),
        ("b".to_string(), serde_json::json!(1)),
    ]);
    let mut value = serde_json::json!("{{a}}");

    substitute_templates(&mut value, &vars);

    assert_eq!(
        value,
        serde_json::json!(1),
        "chained template should fully resolve"
    );
}

#[test]
fn substitute_templates_stops_whole_node_cycles() {
    let vars = HashMap::from([
        ("a".to_string(), serde_json::json!("{{b}}")),
        ("b".to_string(), serde_json::json!("{{a}}")),
    ]);
    let mut value = serde_json::json!("{{a}}");

    substitute_templates(&mut value, &vars);

    assert_eq!(value, serde_json::json!("{{b}}"));
}

#[rstest]
fn substitute_templates_preserves_extracted_null_values(mut tool_message_fixture: ChatMessage) {
    tool_message_fixture.content = serde_json::json!({"optional": null}).to_string();
    let messages = [tool_message_fixture];
    let vars = extract_tool_result_vars(&messages).expect("valid JSON should parse");
    let mut value = serde_json::json!({"optional": "{{call.optional}}"});

    substitute_templates(&mut value, &vars);

    assert_eq!(value, serde_json::json!({"optional": null}));
}

#[rstest]
#[case(
    serde_json::json!({
        "items": [
            {"plain": "value"},
            {"templated": "prefix {{call.value}} suffix"}
        ]
    }),
    true
)]
#[case(serde_json::json!({"items": [{"plain": "value"}]}), false)]
fn has_template_marker_detects_nested_placeholders(
    #[case] value: serde_json::Value,
    #[case] expected: bool,
) {
    assert_eq!(has_template_marker(&value), expected);
}

#[rstest]
fn extract_tool_result_vars_flattens_non_object_roots(mut tool_message_fixture: ChatMessage) {
    tool_message_fixture.content = serde_json::json!(["alpha", true]).to_string();
    tool_message_fixture.tool_call_id = Some("call_array".to_string());
    let messages = [tool_message_fixture];

    let vars = extract_tool_result_vars(&messages).expect("valid JSON should parse");

    assert_eq!(vars.get("call_array.0"), Some(&serde_json::json!("alpha")));
    assert_eq!(vars.get("call_array.1"), Some(&serde_json::json!(true)));
}

#[rstest]
fn extract_tool_result_vars_errors_on_invalid_json(mut tool_message_fixture: ChatMessage) {
    tool_message_fixture.content = "not json {{{".to_string();
    tool_message_fixture.tool_call_id = Some("call_bad".to_string());
    let messages = [tool_message_fixture];

    let result = extract_tool_result_vars(&messages);

    assert!(result.is_err());
    let err = result.expect_err("invalid JSON should return parse error");
    assert_eq!(err.call_id, "call_bad");
}
