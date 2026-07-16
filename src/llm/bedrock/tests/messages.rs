//! Unit tests for Bedrock message conversion: system extraction, role
//! alternation, tool-result merging, and full tool round trips.

use aws_sdk_bedrockruntime::types::ConversationRole;

use crate::llm::bedrock::convert::convert_messages;
use crate::llm::bedrock::documents::document_to_json;
use crate::llm::provider::{ChatMessage, Role};

#[test]
fn test_convert_messages_system_extraction() {
    let messages = vec![
        ChatMessage::system("You are helpful."),
        ChatMessage::system("Be concise."),
        ChatMessage::user("Hello"),
    ];

    let (system, msgs) = convert_messages(&messages).unwrap();

    assert_eq!(system.len(), 2);
    assert_eq!(msgs.len(), 1);
    assert_eq!(*msgs[0].role(), ConversationRole::User);
}

#[test]
fn test_convert_messages_basic_conversation() {
    let messages = vec![
        ChatMessage::user("Hi"),
        ChatMessage::assistant("Hello!"),
        ChatMessage::user("How are you?"),
    ];

    let (system, msgs) = convert_messages(&messages).unwrap();

    assert!(system.is_empty());
    assert_eq!(msgs.len(), 3);
    assert_eq!(*msgs[0].role(), ConversationRole::User);
    assert_eq!(*msgs[1].role(), ConversationRole::Assistant);
    assert_eq!(*msgs[2].role(), ConversationRole::User);
}

#[test]
fn test_convert_messages_tool_results_merge_into_user() {
    let tc = crate::llm::provider::ToolCall {
        id: "call_1".to_string(),
        name: "echo".to_string(),
        arguments: serde_json::json!({"text": "hi"}),
    };
    let tc2 = crate::llm::provider::ToolCall {
        id: "call_2".to_string(),
        name: "time".to_string(),
        arguments: serde_json::json!({}),
    };

    let messages = vec![
        ChatMessage::user("Do things"),
        ChatMessage::assistant_with_tool_calls(None, vec![tc, tc2]),
        ChatMessage::tool_result("call_1", "echo", "hi back"),
        ChatMessage::tool_result("call_2", "time", "12:00"),
    ];

    let (_, msgs) = convert_messages(&messages).unwrap();

    // user, assistant (with tool_use), user (with merged tool_results)
    assert_eq!(msgs.len(), 3);
    assert_eq!(*msgs[2].role(), ConversationRole::User);
    // The merged user message should have 2 content blocks (both ToolResult)
    assert_eq!(msgs[2].content().len(), 2);
    assert!(msgs[2].content()[0].is_tool_result());
    assert!(msgs[2].content()[1].is_tool_result());
}

#[test]
fn test_convert_messages_consecutive_users_merge() {
    let messages = vec![ChatMessage::user("First"), ChatMessage::user("Second")];

    let (_, msgs) = convert_messages(&messages).unwrap();

    // Should merge into a single User message with 2 text blocks
    assert_eq!(msgs.len(), 1);
    assert_eq!(*msgs[0].role(), ConversationRole::User);
    assert_eq!(msgs[0].content().len(), 2);
}

#[test]
fn test_convert_messages_assistant_with_tool_calls() {
    let tc = crate::llm::provider::ToolCall {
        id: "call_1".to_string(),
        name: "search".to_string(),
        arguments: serde_json::json!({"query": "test"}),
    };

    let messages = vec![
        ChatMessage::user("Search for test"),
        ChatMessage::assistant_with_tool_calls(Some("Let me search.".to_string()), vec![tc]),
    ];

    let (_, msgs) = convert_messages(&messages).unwrap();

    assert_eq!(msgs.len(), 2);
    assert_eq!(*msgs[1].role(), ConversationRole::Assistant);
    // Should have text + tool_use
    assert_eq!(msgs[1].content().len(), 2);
    assert!(msgs[1].content()[0].is_text());
    assert!(msgs[1].content()[1].is_tool_use());
}

#[test]
fn test_convert_messages_empty_assistant_content_with_tool_calls() {
    let tc = crate::llm::provider::ToolCall {
        id: "call_1".to_string(),
        name: "echo".to_string(),
        arguments: serde_json::json!({}),
    };

    let messages = vec![
        ChatMessage::user("Go"),
        ChatMessage::assistant_with_tool_calls(None, vec![tc]),
    ];

    let (_, msgs) = convert_messages(&messages).unwrap();

    assert_eq!(msgs.len(), 2);
    // Empty text should not add a Text block
    let assistant_content = msgs[1].content();
    assert_eq!(assistant_content.len(), 1);
    assert!(assistant_content[0].is_tool_use());
}

#[test]
fn test_convert_messages_tool_result_after_regular_user() {
    // Edge case: tool result appears after a user message (from sanitize_tool_messages rewrite)
    // This shouldn't happen normally but we should handle it gracefully
    let messages = vec![
        ChatMessage::user("Hello"),
        ChatMessage {
            role: Role::Tool,
            content: "result".to_string(),
            tool_call_id: Some("call_1".to_string()),
            name: Some("echo".to_string()),
            tool_calls: None,
            content_parts: Vec::new(),
        },
    ];

    let (_, msgs) = convert_messages(&messages).unwrap();

    // User + tool result (as user) = should merge into one User message
    assert_eq!(msgs.len(), 1);
    assert_eq!(*msgs[0].role(), ConversationRole::User);
}

#[test]
fn test_full_tool_round_trip_conversation() {
    // Simulate a complete tool-use conversation:
    // system → user → assistant(tool_calls) → tool_results → user follow-up
    let tc1 = crate::llm::provider::ToolCall {
        id: "call_abc".to_string(),
        name: "get_weather".to_string(),
        arguments: serde_json::json!({"city": "NYC"}),
    };
    let tc2 = crate::llm::provider::ToolCall {
        id: "call_def".to_string(),
        name: "get_time".to_string(),
        arguments: serde_json::json!({"tz": "EST"}),
    };

    let messages = vec![
        ChatMessage::system("You are a helpful assistant."),
        ChatMessage::user("What's the weather and time in NYC?"),
        ChatMessage::assistant_with_tool_calls(
            Some("Let me check both.".to_string()),
            vec![tc1, tc2],
        ),
        ChatMessage::tool_result("call_abc", "get_weather", "72°F and sunny"),
        ChatMessage::tool_result("call_def", "get_time", "3:45 PM EST"),
        ChatMessage::user("Thanks! What about tomorrow?"),
    ];

    let (system, msgs) = convert_messages(&messages).unwrap();

    // 1 system block
    assert_eq!(system.len(), 1);

    // Messages: user, assistant(text+2 tool_use), user(2 tool_results + follow-up text merged)
    // The follow-up user message "Thanks!" merges into the tool_results User message
    // because Bedrock requires strict user/assistant alternation.
    assert_eq!(msgs.len(), 3);

    // msg[0]: user "What's the weather..."
    assert_eq!(*msgs[0].role(), ConversationRole::User);
    assert_eq!(msgs[0].content().len(), 1);
    assert!(msgs[0].content()[0].is_text());

    // msg[1]: assistant with text + 2 tool_use blocks
    assert_eq!(*msgs[1].role(), ConversationRole::Assistant);
    assert_eq!(msgs[1].content().len(), 3); // text + 2 tool_use
    assert!(msgs[1].content()[0].is_text());
    assert!(msgs[1].content()[1].is_tool_use());
    assert!(msgs[1].content()[2].is_tool_use());

    // Verify tool_use IDs and arguments survived conversion
    let tu1 = msgs[1].content()[1].as_tool_use().unwrap();
    assert_eq!(tu1.tool_use_id(), "call_abc");
    assert_eq!(tu1.name(), "get_weather");
    let args1 = document_to_json(tu1.input());
    assert_eq!(args1, serde_json::json!({"city": "NYC"}));

    let tu2 = msgs[1].content()[2].as_tool_use().unwrap();
    assert_eq!(tu2.tool_use_id(), "call_def");
    assert_eq!(tu2.name(), "get_time");

    // msg[2]: user with 2 tool_result blocks + merged follow-up text
    // Tool results are User-role, and "Thanks!" is also User-role, so they merge.
    assert_eq!(*msgs[2].role(), ConversationRole::User);
    assert_eq!(msgs[2].content().len(), 3); // 2 tool_results + 1 text
    assert!(msgs[2].content()[0].is_tool_result());
    assert!(msgs[2].content()[1].is_tool_result());
    assert!(msgs[2].content()[2].is_text());

    // Verify tool_result IDs and content
    let tr1 = msgs[2].content()[0].as_tool_result().unwrap();
    assert_eq!(tr1.tool_use_id(), "call_abc");
    assert_eq!(tr1.content().len(), 1);

    let tr2 = msgs[2].content()[1].as_tool_result().unwrap();
    assert_eq!(tr2.tool_use_id(), "call_def");
}

#[test]
fn test_convert_messages_empty_input() {
    let (system, msgs) = convert_messages(&[]).unwrap();
    assert!(system.is_empty());
    assert!(msgs.is_empty());
}

#[test]
fn test_convert_messages_system_only() {
    let messages = vec![ChatMessage::system("You are helpful.")];
    let (system, msgs) = convert_messages(&messages).unwrap();
    assert_eq!(system.len(), 1);
    assert!(msgs.is_empty());
}

#[test]
fn test_empty_messages_returns_error() {
    let messages = vec![ChatMessage::system("System only, no user messages")];
    let (_, bedrock_msgs) = convert_messages(&messages).unwrap();
    assert!(bedrock_msgs.is_empty());
}
