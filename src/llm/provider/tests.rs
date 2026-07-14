//! Unit tests for provider request shaping and tool-result handling.

use super::*;
mod default_contracts;

fn assert_preserved_tool_result(
    message: &ChatMessage,
    call_id: &str,
    tool_name: &str,
    content: &str,
) {
    assert_eq!(message.role, Role::Tool);
    assert_eq!(message.tool_call_id, Some(call_id.to_string()));
    assert_eq!(message.name, Some(tool_name.to_string()));
    assert_eq!(message.content, content);
}

fn assert_rewritten_orphaned_tool_result(
    message: &ChatMessage,
    tool_name: &str,
    content_fragment: &str,
) {
    assert_eq!(message.role, Role::User);
    assert!(
        message
            .content
            .contains(&format!("[Tool `{tool_name}` returned:"))
    );
    assert!(message.content.contains(content_fragment));
    assert!(message.tool_call_id.is_none());
    assert!(message.name.is_none());
}

#[test]
fn test_sanitize_preserves_valid_pairs() {
    let tc = ToolCall {
        id: "call_1".to_string(),
        name: "echo".to_string(),
        arguments: serde_json::json!({}),
    };
    let mut messages = vec![
        ChatMessage::user("hello"),
        ChatMessage::assistant_with_tool_calls(None, vec![tc]),
        ChatMessage::tool_result("call_1", "echo", "result"),
    ];
    sanitize_tool_messages(&mut messages);
    assert_preserved_tool_result(&messages[2], "call_1", "echo", "result");
}

#[test]
fn test_sanitize_rewrites_orphaned_tool_result() {
    let mut messages = vec![
        ChatMessage::user("hello"),
        ChatMessage::assistant("I'll use a tool"),
        ChatMessage::tool_result("call_missing", "search", "some result"),
    ];
    sanitize_tool_messages(&mut messages);
    assert_eq!(messages[2].role, Role::User);
    assert!(messages[2].content.contains("[Tool `search` returned:"));
    assert!(messages[2].tool_call_id.is_none());
    assert!(messages[2].name.is_none());
}

#[test]
fn test_sanitize_handles_no_tool_messages() {
    let mut messages = vec![
        ChatMessage::system("prompt"),
        ChatMessage::user("hello"),
        ChatMessage::assistant("hi"),
    ];
    let original_len = messages.len();
    sanitize_tool_messages(&mut messages);
    assert_eq!(messages.len(), original_len);
}

#[test]
fn test_sanitize_multiple_orphaned() {
    let tc = ToolCall {
        id: "call_1".to_string(),
        name: "echo".to_string(),
        arguments: serde_json::json!({}),
    };
    let mut messages = vec![
        ChatMessage::user("test"),
        ChatMessage::assistant_with_tool_calls(None, vec![tc]),
        ChatMessage::tool_result("call_1", "echo", "ok"),
        // These are orphaned (call_2 and call_3 have no matching assistant message)
        ChatMessage::tool_result("call_2", "search", "orphan 1"),
        ChatMessage::tool_result("call_3", "http", "orphan 2"),
    ];
    sanitize_tool_messages(&mut messages);
    assert_eq!(messages[2].role, Role::Tool); // call_1 is valid
    assert_eq!(messages[3].role, Role::User); // call_2 orphaned
    assert_eq!(messages[4].role, Role::User); // call_3 orphaned
}

/// Regression: worker's select_tools/execute_plan now emit
/// assistant_with_tool_calls before tool_result messages.
/// Verify sanitize_tool_messages preserves all tool_results when
/// each has a matching assistant tool_call.
#[test]
fn test_sanitize_preserves_tool_results_with_matching_assistant() {
    let tc1 = ToolCall {
        id: "call_sel_1".to_string(),
        name: "search".to_string(),
        arguments: serde_json::json!({"q": "test"}),
    };
    let tc2 = ToolCall {
        id: "call_sel_2".to_string(),
        name: "http".to_string(),
        arguments: serde_json::json!({"url": "https://example.com"}),
    };
    let mut messages = vec![
        ChatMessage::system("You are a helpful assistant."),
        ChatMessage::assistant_with_tool_calls(None, vec![tc1, tc2]),
        ChatMessage::tool_result("call_sel_1", "search", "found 3 results"),
        ChatMessage::tool_result("call_sel_2", "http", "200 OK"),
    ];
    sanitize_tool_messages(&mut messages);

    // All tool_results must keep Role::Tool -- none should be rewritten.
    assert_preserved_tool_result(&messages[2], "call_sel_1", "search", "found 3 results");
    assert_preserved_tool_result(&messages[3], "call_sel_2", "http", "200 OK");
}

/// Regression: the OLD buggy worker code pushed tool_result messages
/// without a preceding assistant_with_tool_calls, causing
/// sanitize_tool_messages to rewrite them as orphaned user messages.
/// This test reproduces that buggy sequence and confirms the rewrite.
#[test]
fn test_sanitize_rewrites_orphaned_tool_results() {
    let mut messages = vec![
        ChatMessage::system("You are a helpful assistant."),
        // No assistant_with_tool_calls -- mimics the old bug.
        ChatMessage::tool_result("call_bug_1", "search", "found 3 results"),
        ChatMessage::tool_result("call_bug_2", "http", "200 OK"),
    ];
    sanitize_tool_messages(&mut messages);

    // Both tool_results must be rewritten to Role::User.
    assert_rewritten_orphaned_tool_result(&messages[1], "search", "found 3 results");
    assert_rewritten_orphaned_tool_result(&messages[2], "http", "200 OK");
}
