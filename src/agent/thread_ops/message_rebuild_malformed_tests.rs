//! Regression tests ensuring malformed `tool_calls` rows are skipped
//! entirely during chat message rebuilding: bad JSON, non-array payloads,
//! boundary rows, and entries with blank/null/non-string `name` or
//! `call_id` fields.

use super::*;

#[rstest]
fn test_rebuild_chat_messages_malformed_tool_calls_json(test_safety_layer: SafetyLayer) {
    let safety = test_safety_layer;
    let messages = vec![
        make_db_msg("user", "Hi"),
        make_db_msg("tool_calls", "not valid json"),
        make_db_msg("assistant", "Done"),
    ];
    let result = rebuild_chat_messages_from_db(&messages, &safety);
    // Malformed JSON is skipped with a warning (logs message_id and parse error)
    assert_user_assistant_with_content(&result, "Hi", "Done");
}

#[rstest]
#[case::object(r#"{"name":"search"}"#)]
#[case::number("42")]
#[case::null("null")]
fn test_rebuild_chat_messages_non_array_tool_calls_json(
    test_safety_layer: SafetyLayer,
    #[case] non_array_json: &str,
) {
    let safety = test_safety_layer;
    let messages = vec![
        make_db_msg("user", "Hi"),
        make_db_msg("tool_calls", non_array_json),
        make_db_msg("assistant", "Done"),
    ];
    let result = rebuild_chat_messages_from_db(&messages, &safety);
    // Non-array JSON for tool_calls is skipped (expecting an array)
    assert_only_user_and_assistant(&result);
    // Also verify message content wasn't modified
    assert_eq!(result[0].content, "Hi");
    assert_eq!(result[1].content, "Done");
}

#[rstest]
#[case::leading(
    {
        let tool_json = serde_json::json!([
            {"name": "echo", "result_preview": "hello"}
        ]);
        vec![
            make_db_msg("tool_calls", &tool_json.to_string()),
            make_db_msg("assistant", "Done"),
        ]
    },
    vec![crate::llm::Role::Assistant],
    vec!["Done"]
)]
#[case::trailing(
    {
        let tool_json = serde_json::json!([
            {"name": "echo", "result_preview": "hello"}
        ]);
        vec![
            make_db_msg("user", "Hi"),
            make_db_msg("tool_calls", &tool_json.to_string()),
        ]
    },
    vec![crate::llm::Role::User],
    vec!["Hi"]
)]
fn test_rebuild_chat_messages_malformed_tool_calls_boundary(
    test_safety_layer: SafetyLayer,
    #[case] messages: Vec<ConversationMessage>,
    #[case] expected_roles: Vec<crate::llm::Role>,
    #[case] expected_contents: Vec<&str>,
) {
    assert_malformed_tool_calls_boundary(
        &test_safety_layer,
        messages,
        &expected_roles,
        &expected_contents,
    );
}

/// Regression tests for malformed tool_calls entries that must be skipped.
/// Before fixes, these were silently processed with fallback values or partial
/// data.
#[rstest]
#[case::missing_name(serde_json::json!([
    {"call_id": "call_0", "parameters": {"q": "x"}, "result": "ok"}
]))]
#[case::mixed_valid_invalid(serde_json::json!([
    {"name": "search", "call_id": "call_0", "parameters": {}, "result": "found"},
    {"name": "write", "parameters": {"path": "a.txt"}, "result": "ok"}
]))]
#[case::null_fields(serde_json::json!([
    {"name": null, "call_id": "call_0", "parameters": {}, "result": "ok"}
]))]
#[case::empty_call_id(serde_json::json!([
    {"name": "search", "call_id": "", "parameters": {}, "result": "ok"}
]))]
#[case::empty_name(serde_json::json!([
    {"name": "", "call_id": "call_0", "parameters": {}, "result": "ok"}
]))]
#[case::whitespace_call_id(serde_json::json!([
    {"name": "search", "call_id": "   ", "parameters": {}, "result": "ok"}
]))]
#[case::whitespace_name(serde_json::json!([
    {"name": "  \t  ", "call_id": "call_0", "parameters": {}, "result": "ok"}
]))]
// Partial enrichment: one entry has call_id, another doesn't - reject entire row
#[case::partial_enrichment(serde_json::json!([
    {"name": "search", "call_id": "call_0", "parameters": {}, "result": "found"},
    {"name": "echo", "parameters": {}, "result": "hello"}
]))]
// Empty array fast path: should be skipped without processing
#[case::empty_array_skip(serde_json::json!([]))]
fn test_rebuild_skips_malformed_tool_calls(
    test_safety_layer: SafetyLayer,
    #[case] malformed_json: serde_json::Value,
) {
    assert_malformed_tool_calls_skipped(&test_safety_layer, malformed_json);
}
