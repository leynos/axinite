//! Unit tests for thread message rebuilding logic in `thread_ops`, covering
//! `ConversationMessage`, `SafetyConfig`, and `SafetyLayer` interactions.
//!
//! Includes property-based tests (via proptest) that enforce the invariant:
//! any `tool_calls` array containing entries with blank/null/non-string `name`
//! or `call_id` fields must cause the entire row to be skipped.

use super::*;
use crate::config::SafetyConfig;
use crate::history::ConversationMessage;
use crate::safety::SafetyLayer;
use rstest::{fixture, rstest};

fn make_db_msg(role: &str, content: &str) -> ConversationMessage {
    ConversationMessage {
        id: uuid::Uuid::new_v4(),
        role: role.to_string(),
        content: content.to_string(),
        created_at: chrono::Utc::now(),
    }
}

#[fixture]
fn test_safety_layer() -> SafetyLayer {
    SafetyLayer::new(&SafetyConfig {
        injection_check_enabled: false,
        max_output_length: 100_000,
    })
}

/// Asserts the result contains exactly one `User` message followed by one
/// `Assistant` message. Used to verify that a malformed or legacy `tool_calls`
/// row is skipped entirely.
fn assert_only_user_and_assistant(result: &[ChatMessage]) {
    assert_eq!(result.len(), 2);
    assert_eq!(result[0].role, crate::llm::Role::User);
    assert_eq!(result[1].role, crate::llm::Role::Assistant);
}

/// Asserts the result is exactly a `User` message then an `Assistant` message
/// with the specified text content for each.
fn assert_user_assistant_with_content(
    result: &[ChatMessage],
    user_content: &str,
    assistant_content: &str,
) {
    assert_only_user_and_assistant(result);
    assert_eq!(result[0].content, user_content,      "unexpected user content");
    assert_eq!(result[1].content, assistant_content, "unexpected assistant content");
}

/// Assert that a `tool_calls` row whose JSON content is `tool_json` is skipped
/// entirely, leaving only the surrounding user and assistant messages in the
/// output.
fn assert_malformed_tool_calls_skipped(safety: &SafetyLayer, tool_json: serde_json::Value) {
    let messages = vec![
        make_db_msg("user", "Hi"),
        make_db_msg("tool_calls", &tool_json.to_string()),
        make_db_msg("assistant", "Done"),
    ];
    let result = rebuild_chat_messages_from_db(&messages, safety);
    assert_user_assistant_with_content(&result, "Hi", "Done");
}

fn assert_malformed_tool_calls_boundary(
    safety: &SafetyLayer,
    messages: Vec<ConversationMessage>,
    expected_roles: &[crate::llm::Role],
    expected_contents: &[&str],
) {
    let result = rebuild_chat_messages_from_db(&messages, safety);

    assert_eq!(result.len(), expected_roles.len());
    assert_eq!(expected_contents.len(), result.len());
    for (message, expected_role) in result.iter().zip(expected_roles) {
        assert_eq!(&message.role, expected_role);
    }
    for (message, expected_content) in result.iter().zip(expected_contents) {
        assert_eq!(message.content, *expected_content);
    }
}

/// Asserts that `content` contains a well-formed `<tool_output name="…">` opening
/// tag for `tool_name` and that every string in `expected_fragments` is present.
fn assert_tool_output_content(content: &str, tool_name: &str, expected_fragments: &[&str]) {
    let opening_tag = format!("<tool_output name=\"{tool_name}\"");
    assert!(
        content.contains(&opening_tag),
        "expected opening tag {opening_tag:?}; content: {content:?}"
    );
    for fragment in expected_fragments {
        assert!(
            content.contains(fragment),
            "expected content to contain {fragment:?}; content: {content:?}"
        );
    }
}

/// Asserts that `msg` is a [`crate::llm::Role::Tool`] message with the given
/// `call_id`, that its content is wrapped in a `<tool_output name="…">` tag
/// for `tool_name`, and that every string in `expected_fragments` appears in
/// the content.
fn assert_tool_result_message(
    msg: &ChatMessage,
    call_id: &str,
    tool_name: &str,
    expected_fragments: &[&str],
) {
    assert_eq!(msg.role, crate::llm::Role::Tool, "expected Tool role");
    assert_eq!(
        msg.tool_call_id,
        Some(call_id.to_string()),
        "expected tool_call_id = {call_id}"
    );
    assert_tool_output_content(&msg.content, tool_name, expected_fragments);
}

/// Asserts that `msg` is an [`crate::llm::Role::Assistant`] message with
/// `tool_calls` populated, and returns a reference to the inner slice.
fn assert_assistant_with_tool_calls(msg: &ChatMessage) -> &[crate::llm::ToolCall] {
    assert_eq!(
        msg.role,
        crate::llm::Role::Assistant,
        "expected Assistant role"
    );
    msg.tool_calls
        .as_deref()
        .expect("expected tool_calls to be Some on assistant message")
}

#[rstest]
fn test_rebuild_chat_messages_user_assistant_only(test_safety_layer: SafetyLayer) {
    let safety = test_safety_layer;
    let messages = vec![
        make_db_msg("user", "Hello"),
        make_db_msg("assistant", "Hi there!"),
    ];
    let result = rebuild_chat_messages_from_db(&messages, &safety);
    assert_only_user_and_assistant(&result);
}

#[rstest]
fn test_rebuild_chat_messages_with_enriched_tool_calls(test_safety_layer: SafetyLayer) {
    let safety = test_safety_layer;
    let tool_json = serde_json::json!([
        {
            "name": "memory_search",
            "call_id": "call_0",
            "parameters": {"query": "test"},
            "result": "Found 3 results",
            "result_preview": "Found 3 re..."
        },
        {
            "name": "echo",
            "call_id": "call_1",
            "parameters": {"message": "hi"},
            "error": "timeout"
        }
    ]);
    let messages = vec![
        make_db_msg("user", "Search for test"),
        make_db_msg("tool_calls", &tool_json.to_string()),
        make_db_msg("assistant", "I found some results."),
    ];
    let result = rebuild_chat_messages_from_db(&messages, &safety);

    assert_eq!(result.len(), 5);
    assert_eq!(result[0].role, crate::llm::Role::User);

    let tcs = assert_assistant_with_tool_calls(&result[1]);
    assert_eq!(tcs.len(), 2);
    assert_eq!(tcs[0].name, "memory_search");
    assert_eq!(tcs[0].id, "call_0");
    assert_eq!(tcs[1].name, "echo");

    assert_tool_result_message(&result[2], "call_0", "memory_search", &["Found 3 results"]);
    assert_tool_result_message(&result[3], "call_1", "echo", &["Error: timeout"]);

    assert_eq!(result[4].role, crate::llm::Role::Assistant);
    assert_eq!(result[4].content, "I found some results.");
}

#[rstest]
fn test_rebuild_chat_messages_legacy_tool_calls_skipped(test_safety_layer: SafetyLayer) {
    // Legacy format: no call_id field
    assert_malformed_tool_calls_skipped(
        &test_safety_layer,
        serde_json::json!([
            {"name": "echo", "result_preview": "hello"}
        ]),
    );
}

#[rstest]
fn test_rebuild_chat_messages_empty(test_safety_layer: SafetyLayer) {
    let safety = test_safety_layer;
    let result = rebuild_chat_messages_from_db(&[], &safety);
    assert!(result.is_empty());
}

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
fn test_rebuild_skips_malformed_tool_calls(
    test_safety_layer: SafetyLayer,
    #[case] malformed_json: serde_json::Value,
) {
    assert_malformed_tool_calls_skipped(&test_safety_layer, malformed_json);
}

#[rstest]
fn test_rebuild_chat_messages_multi_turn_with_tools(test_safety_layer: SafetyLayer) {
    let safety = test_safety_layer;
    let tool_json_1 = serde_json::json!([
        {"name": "search", "call_id": "call_0", "parameters": {}, "result": "found it"}
    ]);
    let tool_json_2 = serde_json::json!([
        {"name": "write", "call_id": "call_1", "parameters": {"path": "a.txt"}, "result": "ok"}
    ]);
    let messages = vec![
        make_db_msg("user", "Find X"),
        make_db_msg("tool_calls", &tool_json_1.to_string()),
        make_db_msg("assistant", "Found X"),
        make_db_msg("user", "Write it"),
        make_db_msg("tool_calls", &tool_json_2.to_string()),
        make_db_msg("assistant", "Written"),
    ];
    let result = rebuild_chat_messages_from_db(&messages, &safety);

    assert_eq!(result.len(), 8);

    // Verify role ordering for all messages
    assert_eq!(result[0].role, crate::llm::Role::User);
    assert_eq!(result[0].content, "Find X");

    assert_eq!(result[1].role, crate::llm::Role::Assistant);
    let first_turn_tool_calls = assert_assistant_with_tool_calls(&result[1]);
    assert_eq!(first_turn_tool_calls.len(), 1);
    assert_eq!(first_turn_tool_calls[0].id, "call_0");
    assert_eq!(first_turn_tool_calls[0].name, "search");

    assert_eq!(result[2].role, crate::llm::Role::Tool);
    assert_tool_result_message(&result[2], "call_0", "search", &["found it"]);

    assert_eq!(result[3].role, crate::llm::Role::Assistant);
    assert_eq!(result[3].content, "Found X");

    assert_eq!(result[4].role, crate::llm::Role::User);
    assert_eq!(result[4].content, "Write it");

    assert_eq!(result[5].role, crate::llm::Role::Assistant);
    let second_turn_tool_calls = assert_assistant_with_tool_calls(&result[5]);
    assert_eq!(second_turn_tool_calls.len(), 1);
    assert_eq!(second_turn_tool_calls[0].id, "call_1");
    assert_eq!(second_turn_tool_calls[0].name, "write");

    assert_eq!(result[6].role, crate::llm::Role::Tool);
    assert_tool_result_message(&result[6], "call_1", "write", &["ok"]);

    assert_eq!(result[7].role, crate::llm::Role::Assistant);
    assert_eq!(result[7].content, "Written");
}

#[rstest]
#[case::result_preview(
    serde_json::json!([
        {
            "name": "search",
            "call_id": "call_preview",
            "parameters": {"q": "test"},
            "result_preview": "Preview of search results…"
        }
    ]),
    "Search",
    "call_preview",
    "search",
    "Preview of search results"
)]
#[case::defaults_to_ok(
    serde_json::json!([
        {
            "name": "noop",
            "call_id": "call_ok",
            "parameters": {}
        }
    ]),
    "Run noop",
    "call_ok",
    "noop",
    "OK"
)]
fn test_tool_result_content_fallbacks(
    test_safety_layer: SafetyLayer,
    #[case] tool_json: serde_json::Value,
    #[case] user_content: &str,
    #[case] expected_call_id: &str,
    #[case] expected_tool_name: &str,
    #[case] expected_fragment: &str,
) {
    let messages = vec![
        make_db_msg("user", user_content),
        make_db_msg("tool_calls", &tool_json.to_string()),
        make_db_msg("assistant", "Done"),
    ];
    let result = rebuild_chat_messages_from_db(&messages, &test_safety_layer);

    assert_eq!(result.len(), 4);
    let tcs = assert_assistant_with_tool_calls(&result[1]);
    assert_eq!(tcs.len(), 1);
    assert_eq!(tcs[0].id, expected_call_id);
    assert_eq!(tcs[0].name, expected_tool_name);
    assert_tool_result_message(
        &result[2],
        expected_call_id,
        expected_tool_name,
        &[expected_fragment],
    );
}

#[cfg(test)]
mod property_tests {
    use super::*;
    use proptest::prelude::*;

    /// Generates a `serde_json::Value` for a single tool-call entry
    /// where `name` is blank, null, or a non-string type.
    fn bad_name_strategy() -> impl Strategy<Value = serde_json::Value> {
        prop_oneof![
            Just(serde_json::Value::Null),
            Just(serde_json::Value::Bool(false)),
            Just(serde_json::Value::from(0i64)),
            "[ \t]*".prop_map(serde_json::Value::String), // blank/whitespace
        ]
        .prop_map(|bad_name| {
            serde_json::json!({
                "name": bad_name,
                "call_id": "call_0",
                "parameters": {},
                "result": "ok"
            })
        })
    }

    /// Generates a `serde_json::Value` for a single tool-call entry
    /// where `call_id` is blank, null, or a non-string type.
    fn bad_call_id_strategy() -> impl Strategy<Value = serde_json::Value> {
        prop_oneof![
            Just(serde_json::Value::Null),
            Just(serde_json::Value::Bool(true)),
            Just(serde_json::Value::from(42i64)),
            "[ \t]*".prop_map(serde_json::Value::String),
        ]
        .prop_map(|bad_id| {
            serde_json::json!({
                "name": "search",
                "call_id": bad_id,
                "parameters": {},
                "result": "ok"
            })
        })
    }

    proptest! {
        #[test]
        fn prop_bad_name_entry_skips_row(bad_entry in bad_name_strategy()) {
            let safety = SafetyLayer::new(&SafetyConfig {
                injection_check_enabled: false,
                max_output_length: 100_000,
            });
            let arr = serde_json::Value::Array(vec![bad_entry]);
            assert_malformed_tool_calls_skipped(&safety, arr);
        }

        #[test]
        fn prop_bad_call_id_entry_skips_row(bad_entry in bad_call_id_strategy()) {
            let safety = SafetyLayer::new(&SafetyConfig {
                injection_check_enabled: false,
                max_output_length: 100_000,
            });
            let arr = serde_json::Value::Array(vec![bad_entry]);
            assert_malformed_tool_calls_skipped(&safety, arr);
        }
    }
}
