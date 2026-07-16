//! Tests for `ChatMessage` conversion and tool-message flattening.

use super::super::wire::*;
use crate::llm::provider::{ChatMessage, ToolCall};

#[test]
fn test_message_conversion() {
    let msg = ChatMessage::user("Hello");
    let chat_msg: ChatCompletionMessage = msg.into();
    assert_eq!(chat_msg.role, "user");
    assert_eq!(
        chat_msg.content.as_ref().and_then(|c| c.as_text()),
        Some("Hello")
    );
}

#[test]
fn test_tool_message_conversion() {
    let msg = ChatMessage::tool_result("call_123", "my_tool", "result");
    let chat_msg: ChatCompletionMessage = msg.into();
    assert_eq!(chat_msg.role, "tool");
    assert_eq!(chat_msg.tool_call_id, Some("call_123".to_string()));
    assert_eq!(chat_msg.name, Some("my_tool".to_string()));
}

#[test]
fn test_assistant_with_tool_calls_conversion() {
    use crate::llm::ToolCall;

    let tool_calls = vec![
        ToolCall {
            id: "call_1".to_string(),
            name: "list_issues".to_string(),
            arguments: serde_json::json!({"owner": "foo", "repo": "bar"}),
        },
        ToolCall {
            id: "call_2".to_string(),
            name: "search".to_string(),
            arguments: serde_json::json!({"query": "test"}),
        },
    ];

    let msg = ChatMessage::assistant_with_tool_calls(None, tool_calls);
    let chat_msg: ChatCompletionMessage = msg.into();

    assert_eq!(chat_msg.role, "assistant");

    let tc = chat_msg.tool_calls.expect("tool_calls present");
    assert_eq!(tc.len(), 2);
    assert_eq!(tc[0].id, "call_1");
    assert_eq!(tc[0].function.name, "list_issues");
    assert_eq!(tc[0].call_type, "function");
    assert_eq!(tc[1].id, "call_2");
    assert_eq!(tc[1].function.name, "search");
}

#[test]
fn test_assistant_without_tool_calls_has_none() {
    let msg = ChatMessage::assistant("Hello");
    let chat_msg: ChatCompletionMessage = msg.into();
    assert!(chat_msg.tool_calls.is_none());
}

#[test]
fn test_tool_call_arguments_serialized_to_string() {
    use crate::llm::ToolCall;

    let tc = ToolCall {
        id: "call_1".to_string(),
        name: "test".to_string(),
        arguments: serde_json::json!({"key": "value"}),
    };
    let msg = ChatMessage::assistant_with_tool_calls(None, vec![tc]);
    let chat_msg: ChatCompletionMessage = msg.into();

    let calls = chat_msg.tool_calls.unwrap();
    // Arguments should be a JSON string, not a nested object
    let parsed: serde_json::Value =
        serde_json::from_str(&calls[0].function.arguments).expect("valid JSON string");
    assert_eq!(parsed["key"], "value");
}

#[test]
fn test_flatten_no_tool_messages_passthrough() {
    let messages = vec![
        ChatCompletionMessage {
            role: "system".to_string(),
            content: Some(MessageContent::Text("You are helpful.".to_string())),
            tool_call_id: None,
            name: None,
            tool_calls: None,
        },
        ChatCompletionMessage {
            role: "user".to_string(),
            content: Some(MessageContent::Text("Hello".to_string())),
            tool_call_id: None,
            name: None,
            tool_calls: None,
        },
    ];
    let result = flatten_tool_messages(messages);
    assert_eq!(result.len(), 2);
    assert_eq!(result[0].role, "system");
    assert_eq!(result[1].role, "user");
}

#[test]
fn test_flatten_tool_call_and_result() {
    let messages = vec![
        ChatCompletionMessage {
            role: "user".to_string(),
            content: Some(MessageContent::Text("test".to_string())),
            tool_call_id: None,
            name: None,
            tool_calls: None,
        },
        ChatCompletionMessage {
            role: "assistant".to_string(),
            content: None,
            tool_call_id: None,
            name: None,
            tool_calls: Some(vec![ChatCompletionToolCall {
                id: "call_1".to_string(),
                call_type: "function".to_string(),
                function: ChatCompletionToolCallFunction {
                    name: "echo".to_string(),
                    arguments: r#"{"message":"hi"}"#.to_string(),
                },
            }]),
        },
        ChatCompletionMessage {
            role: "tool".to_string(),
            content: Some(MessageContent::Text("hi".to_string())),
            tool_call_id: Some("call_1".to_string()),
            name: Some("echo".to_string()),
            tool_calls: None,
        },
    ];

    let result = flatten_tool_messages(messages);
    assert_eq!(result.len(), 3);

    // Assistant tool_calls → plain assistant text
    assert_eq!(result[1].role, "assistant");
    assert!(result[1].tool_calls.is_none());
    assert!(
        result[1]
            .content
            .as_ref()
            .and_then(|c| c.as_text())
            .unwrap()
            .contains("[Called tool `echo`")
    );

    // Tool result → user message
    assert_eq!(result[2].role, "user");
    assert!(result[2].tool_call_id.is_none());
    assert!(
        result[2]
            .content
            .as_ref()
            .and_then(|c| c.as_text())
            .unwrap()
            .contains("[Tool `echo` returned: hi]")
    );
}

#[test]
fn test_flatten_preserves_assistant_text_with_tool_calls() {
    let messages = vec![
        ChatCompletionMessage {
            role: "assistant".to_string(),
            content: Some(MessageContent::Text("Let me check that.".to_string())),
            tool_call_id: None,
            name: None,
            tool_calls: Some(vec![ChatCompletionToolCall {
                id: "call_1".to_string(),
                call_type: "function".to_string(),
                function: ChatCompletionToolCallFunction {
                    name: "search".to_string(),
                    arguments: r#"{"q":"test"}"#.to_string(),
                },
            }]),
        },
        ChatCompletionMessage {
            role: "tool".to_string(),
            content: Some(MessageContent::Text("found it".to_string())),
            tool_call_id: Some("call_1".to_string()),
            name: Some("search".to_string()),
            tool_calls: None,
        },
    ];

    let result = flatten_tool_messages(messages);
    let text = result[0]
        .content
        .as_ref()
        .and_then(|c| c.as_text())
        .unwrap();
    assert!(text.starts_with("Let me check that."));
    assert!(text.contains("[Called tool `search`"));
}

// -- flatten_tool_messages edge cases -------------------------------------

#[test]
fn test_flatten_tool_result_missing_name_uses_unknown() {
    let messages = vec![ChatCompletionMessage {
        role: "tool".to_string(),
        content: Some(MessageContent::Text("result data".to_string())),
        tool_call_id: Some("call_1".to_string()),
        name: None,
        tool_calls: None,
    }];
    let result = flatten_tool_messages(messages);
    assert_eq!(result[0].role, "user");
    assert!(
        result[0]
            .content
            .as_ref()
            .unwrap()
            .as_text()
            .unwrap()
            .contains("[Tool `unknown` returned:")
    );
}

#[test]
fn test_flatten_tool_result_missing_content_uses_empty() {
    let messages = vec![ChatCompletionMessage {
        role: "tool".to_string(),
        content: None,
        tool_call_id: Some("call_1".to_string()),
        name: Some("my_tool".to_string()),
        tool_calls: None,
    }];
    let result = flatten_tool_messages(messages);
    assert_eq!(result[0].role, "user");
    assert!(
        result[0]
            .content
            .as_ref()
            .unwrap()
            .as_text()
            .unwrap()
            .contains("[Tool `my_tool` returned: ]")
    );
}

#[test]
fn test_flatten_multiple_tool_calls_in_single_assistant_message() {
    let messages = vec![
        ChatCompletionMessage {
            role: "assistant".to_string(),
            content: None,
            tool_call_id: None,
            name: None,
            tool_calls: Some(vec![
                ChatCompletionToolCall {
                    id: "call_1".to_string(),
                    call_type: "function".to_string(),
                    function: ChatCompletionToolCallFunction {
                        name: "search".to_string(),
                        arguments: r#"{"q":"a"}"#.to_string(),
                    },
                },
                ChatCompletionToolCall {
                    id: "call_2".to_string(),
                    call_type: "function".to_string(),
                    function: ChatCompletionToolCallFunction {
                        name: "fetch".to_string(),
                        arguments: r#"{"url":"http://x"}"#.to_string(),
                    },
                },
            ]),
        },
        ChatCompletionMessage {
            role: "tool".to_string(),
            content: Some(MessageContent::Text("found".to_string())),
            tool_call_id: Some("call_1".to_string()),
            name: Some("search".to_string()),
            tool_calls: None,
        },
        ChatCompletionMessage {
            role: "tool".to_string(),
            content: Some(MessageContent::Text("fetched".to_string())),
            tool_call_id: Some("call_2".to_string()),
            name: Some("fetch".to_string()),
            tool_calls: None,
        },
    ];
    let result = flatten_tool_messages(messages);
    assert_eq!(result.len(), 3);
    // Assistant message has both calls described
    let assistant_text = result[0].content.as_ref().unwrap().as_text().unwrap();
    assert!(assistant_text.contains("[Called tool `search`"));
    assert!(assistant_text.contains("[Called tool `fetch`"));
    assert!(result[0].tool_calls.is_none());
    // Both tool results become user messages
    assert_eq!(result[1].role, "user");
    assert_eq!(result[2].role, "user");
}

// -- ChatMessage → ChatCompletionMessage edge cases -----------------------

#[test]
fn test_assistant_empty_content_with_tool_calls_becomes_none() {
    // When content is empty string and tool_calls are present, content
    // should be None to avoid sending `"content": ""` which some APIs reject.
    let msg = ChatMessage::assistant_with_tool_calls(
        None,
        vec![ToolCall {
            id: "call_1".to_string(),
            name: "test".to_string(),
            arguments: serde_json::json!({}),
        }],
    );
    let chat_msg: ChatCompletionMessage = msg.into();
    assert!(
        chat_msg.content.is_none(),
        "empty content with tool_calls should serialize as None"
    );
}

#[test]
fn test_system_message_conversion() {
    let msg = ChatMessage::system("You are a helpful assistant.");
    let chat_msg: ChatCompletionMessage = msg.into();
    assert_eq!(chat_msg.role, "system");
    assert_eq!(
        chat_msg.content.as_ref().unwrap().as_text().unwrap(),
        "You are a helpful assistant."
    );
    assert!(chat_msg.tool_calls.is_none());
    assert!(chat_msg.tool_call_id.is_none());
}
