//! Tests for recovering tool calls emitted as XML tags or bracket text in
//! response content instead of the structured tool_calls field.

use super::*;

// ---- recover_tool_calls_from_content tests ----

fn make_tools(names: &[&str]) -> Vec<ToolDefinition> {
    names
        .iter()
        .map(|n| ToolDefinition {
            name: n.to_string(),
            description: String::new(),
            parameters: serde_json::json!({}),
        })
        .collect()
}

#[test]
fn test_recover_bare_tool_name() {
    let tools = make_tools(&["tool_list", "tool_auth"]);
    let content = "<tool_call>tool_list</tool_call>";
    let calls = recover_tool_calls_from_content(content, &tools);
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].name, "tool_list");
    assert_eq!(calls[0].arguments, serde_json::json!({}));
}

#[test]
fn test_recover_json_tool_call() {
    let tools = make_tools(&["memory_search"]);
    let content =
        r#"<tool_call>{"name": "memory_search", "arguments": {"query": "test"}}</tool_call>"#;
    let calls = recover_tool_calls_from_content(content, &tools);
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].name, "memory_search");
    assert_eq!(calls[0].arguments, serde_json::json!({"query": "test"}));
}

#[test]
fn test_recover_pipe_delimited() {
    let tools = make_tools(&["tool_list"]);
    let content = "<|tool_call|>tool_list<|/tool_call|>";
    let calls = recover_tool_calls_from_content(content, &tools);
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].name, "tool_list");
}

#[test]
fn test_recover_unknown_tool_ignored() {
    let tools = make_tools(&["tool_list"]);
    let content = "<tool_call>nonexistent_tool</tool_call>";
    let calls = recover_tool_calls_from_content(content, &tools);
    assert!(calls.is_empty());
}

#[test]
fn test_recover_no_tags() {
    let tools = make_tools(&["tool_list"]);
    let content = "Just a normal response.";
    let calls = recover_tool_calls_from_content(content, &tools);
    assert!(calls.is_empty());
}

#[test]
fn test_recover_multiple_tool_calls() {
    let tools = make_tools(&["tool_list", "tool_auth"]);
    let content = "<tool_call>tool_list</tool_call>\n<tool_call>tool_auth</tool_call>";
    let calls = recover_tool_calls_from_content(content, &tools);
    assert_eq!(calls.len(), 2);
    assert_eq!(calls[0].name, "tool_list");
    assert_eq!(calls[1].name, "tool_auth");
}

#[test]
fn test_recover_function_call_variant() {
    let tools = make_tools(&["shell"]);
    let content = r#"<function_call>{"name": "shell", "arguments": {"cmd": "ls"}}</function_call>"#;
    let calls = recover_tool_calls_from_content(content, &tools);
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].name, "shell");
}

#[test]
fn test_recover_with_surrounding_text() {
    let tools = make_tools(&["tool_list"]);
    let content = "Let me check.\n\n<tool_call>tool_list</tool_call>\n\nDone.";
    let calls = recover_tool_calls_from_content(content, &tools);
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].name, "tool_list");
}

#[test]
fn test_recover_bracket_format_tool_call() {
    let tools = make_tools(&["http"]);
    let content = "Let me try that. [Called tool `http` with arguments: {\"method\":\"GET\",\"url\":\"https://example.com\"}]";
    let calls = recover_tool_calls_from_content(content, &tools);
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].name, "http");
    assert_eq!(calls[0].arguments["method"], "GET");
    assert_eq!(calls[0].arguments["url"], "https://example.com");
}

#[test]
fn test_recover_bracket_format_unknown_tool_ignored() {
    let tools = make_tools(&["http"]);
    let content = "[Called tool `unknown_tool` with arguments: {}]";
    let calls = recover_tool_calls_from_content(content, &tools);
    assert!(calls.is_empty());
}
