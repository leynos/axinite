//! Unit tests for Anthropic OAuth message conversion.

use super::convert::{convert_messages, extract_response_content};
use super::*;
use crate::llm::provider::{ChatMessage, ToolCall};

#[test]
fn test_convert_messages_extracts_system() {
    let messages = vec![
        ChatMessage::system("You are helpful."),
        ChatMessage::user("Hello"),
    ];
    let (system, msgs) = convert_messages(messages);
    assert_eq!(system, Some("You are helpful.".to_string()));
    assert_eq!(msgs.len(), 1);
    assert_eq!(msgs[0].role, "user");
}

#[test]
fn test_convert_messages_multiple_systems() {
    let messages = vec![
        ChatMessage::system("System 1"),
        ChatMessage::system("System 2"),
        ChatMessage::user("Hello"),
    ];
    let (system, msgs) = convert_messages(messages);
    assert_eq!(system, Some("System 1\n\nSystem 2".to_string()));
    assert_eq!(msgs.len(), 1);
}

#[test]
fn test_convert_messages_tool_calls() {
    let tool_calls = vec![ToolCall {
        id: "call_1".to_string(),
        name: "search".to_string(),
        arguments: serde_json::json!({"q": "test"}),
    }];
    let messages = vec![
        ChatMessage::user("Search for test"),
        ChatMessage::assistant_with_tool_calls(Some("Let me search.".to_string()), tool_calls),
        ChatMessage::tool_result("call_1", "search", "found it"),
    ];
    let (system, msgs) = convert_messages(messages);
    assert!(system.is_none());
    assert_eq!(msgs.len(), 3);
    assert_eq!(msgs[0].role, "user");
    assert_eq!(msgs[1].role, "assistant");
    // Tool result should be a user message
    assert_eq!(msgs[2].role, "user");
}

#[test]
fn test_extract_response_text_only() {
    let response = AnthropicResponse {
        content: vec![AnthropicResponseBlock::Text {
            text: "Hello!".to_string(),
        }],
        stop_reason: Some("end_turn".to_string()),
        usage: AnthropicUsage {
            input_tokens: 10,
            output_tokens: 5,
            cache_creation_input_tokens: 0,
            cache_read_input_tokens: 0,
        },
    };
    let (content, tool_calls) = extract_response_content(&response);
    assert_eq!(content, Some("Hello!".to_string()));
    assert!(tool_calls.is_empty());
}

#[test]
fn test_extract_response_with_tool_use() {
    let response = AnthropicResponse {
        content: vec![
            AnthropicResponseBlock::Text {
                text: "Let me search.".to_string(),
            },
            AnthropicResponseBlock::ToolUse {
                id: "call_1".to_string(),
                name: "search".to_string(),
                input: serde_json::json!({"q": "test"}),
            },
        ],
        stop_reason: Some("tool_use".to_string()),
        usage: AnthropicUsage {
            input_tokens: 20,
            output_tokens: 15,
            cache_creation_input_tokens: 0,
            cache_read_input_tokens: 0,
        },
    };
    let (content, tool_calls) = extract_response_content(&response);
    assert_eq!(content, Some("Let me search.".to_string()));
    assert_eq!(tool_calls.len(), 1);
    assert_eq!(tool_calls[0].name, "search");
}
