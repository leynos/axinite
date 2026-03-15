//! Tests for chat history turn reconstruction and thread helpers.

use super::*;

#[test]
fn test_build_turns_from_db_messages_complete() {
    let now = chrono::Utc::now();
    let messages = vec![
        crate::history::ConversationMessage {
            id: Uuid::new_v4(),
            role: "user".to_string(),
            content: "Hello".to_string(),
            created_at: now,
        },
        crate::history::ConversationMessage {
            id: Uuid::new_v4(),
            role: "assistant".to_string(),
            content: "Hi there!".to_string(),
            created_at: now + chrono::TimeDelta::seconds(1),
        },
        crate::history::ConversationMessage {
            id: Uuid::new_v4(),
            role: "user".to_string(),
            content: "How are you?".to_string(),
            created_at: now + chrono::TimeDelta::seconds(2),
        },
        crate::history::ConversationMessage {
            id: Uuid::new_v4(),
            role: "assistant".to_string(),
            content: "Doing well!".to_string(),
            created_at: now + chrono::TimeDelta::seconds(3),
        },
    ];

    let turns = build_turns_from_db_messages(&messages);
    assert_eq!(turns.len(), 2);
    assert_eq!(turns[0].user_input, "Hello");
    assert_eq!(turns[0].response.as_deref(), Some("Hi there!"));
    assert_eq!(turns[0].state, "Completed");
    assert_eq!(turns[1].user_input, "How are you?");
    assert_eq!(turns[1].response.as_deref(), Some("Doing well!"));
}

#[test]
fn test_build_turns_from_db_messages_incomplete_last() {
    let now = chrono::Utc::now();
    let messages = vec![
        crate::history::ConversationMessage {
            id: Uuid::new_v4(),
            role: "user".to_string(),
            content: "Hello".to_string(),
            created_at: now,
        },
        crate::history::ConversationMessage {
            id: Uuid::new_v4(),
            role: "assistant".to_string(),
            content: "Hi!".to_string(),
            created_at: now + chrono::TimeDelta::seconds(1),
        },
        crate::history::ConversationMessage {
            id: Uuid::new_v4(),
            role: "user".to_string(),
            content: "Lost message".to_string(),
            created_at: now + chrono::TimeDelta::seconds(2),
        },
    ];

    let turns = build_turns_from_db_messages(&messages);
    assert_eq!(turns.len(), 2);
    assert_eq!(turns[1].user_input, "Lost message");
    assert!(turns[1].response.is_none());
    assert_eq!(turns[1].state, "Failed");
}

#[test]
fn test_build_turns_with_tool_calls() {
    let now = chrono::Utc::now();
    let tool_calls_json = serde_json::json!([
        {"name": "shell", "result_preview": "file1.txt\nfile2.txt"},
        {"name": "http", "error": "timeout"}
    ]);
    let messages = vec![
        crate::history::ConversationMessage {
            id: Uuid::new_v4(),
            role: "user".to_string(),
            content: "List files".to_string(),
            created_at: now,
        },
        crate::history::ConversationMessage {
            id: Uuid::new_v4(),
            role: "tool_calls".to_string(),
            content: tool_calls_json.to_string(),
            created_at: now + chrono::TimeDelta::milliseconds(500),
        },
        crate::history::ConversationMessage {
            id: Uuid::new_v4(),
            role: "assistant".to_string(),
            content: "Here are the files".to_string(),
            created_at: now + chrono::TimeDelta::seconds(1),
        },
    ];

    let turns = build_turns_from_db_messages(&messages);
    assert_eq!(turns.len(), 1);
    assert_eq!(turns[0].tool_calls.len(), 2);
    assert_eq!(turns[0].tool_calls[0].name, "shell");
    assert!(turns[0].tool_calls[0].has_result);
    assert!(!turns[0].tool_calls[0].has_error);
    assert_eq!(
        turns[0].tool_calls[0].result_preview.as_deref(),
        Some("file1.txt\nfile2.txt")
    );
    assert_eq!(turns[0].tool_calls[1].name, "http");
    assert!(turns[0].tool_calls[1].has_error);
    assert_eq!(turns[0].tool_calls[1].error.as_deref(), Some("timeout"));
    assert_eq!(turns[0].response.as_deref(), Some("Here are the files"));
    assert_eq!(turns[0].state, "Completed");
}

#[test]
fn test_build_turns_with_malformed_tool_calls() {
    let now = chrono::Utc::now();
    let messages = vec![
        crate::history::ConversationMessage {
            id: Uuid::new_v4(),
            role: "user".to_string(),
            content: "Hello".to_string(),
            created_at: now,
        },
        crate::history::ConversationMessage {
            id: Uuid::new_v4(),
            role: "tool_calls".to_string(),
            content: "not valid json".to_string(),
            created_at: now + chrono::TimeDelta::milliseconds(500),
        },
        crate::history::ConversationMessage {
            id: Uuid::new_v4(),
            role: "assistant".to_string(),
            content: "Done".to_string(),
            created_at: now + chrono::TimeDelta::seconds(1),
        },
    ];

    let turns = build_turns_from_db_messages(&messages);
    assert_eq!(turns.len(), 1);
    assert!(turns[0].tool_calls.is_empty());
    assert_eq!(turns[0].response.as_deref(), Some("Done"));
}

#[test]
fn test_build_turns_backward_compatible_no_tool_calls() {
    let now = chrono::Utc::now();
    let messages = vec![
        crate::history::ConversationMessage {
            id: Uuid::new_v4(),
            role: "user".to_string(),
            content: "Hello".to_string(),
            created_at: now,
        },
        crate::history::ConversationMessage {
            id: Uuid::new_v4(),
            role: "assistant".to_string(),
            content: "Hi!".to_string(),
            created_at: now + chrono::TimeDelta::seconds(1),
        },
    ];

    let turns = build_turns_from_db_messages(&messages);
    assert_eq!(turns.len(), 1);
    assert!(turns[0].tool_calls.is_empty());
    assert_eq!(turns[0].response.as_deref(), Some("Hi!"));
    assert_eq!(turns[0].state, "Completed");
}
