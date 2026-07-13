//! Tests for context message building and checkpoint restoration, including
//! tool-call history round-trips (regression coverage for #568).

use uuid::Uuid;

use crate::agent::session::{Thread, ThreadState};
use crate::llm::{ChatMessage, ToolCall};

#[test]
fn test_thread_messages() {
    let mut thread = Thread::new(Uuid::new_v4());

    thread.start_turn("First message");
    thread.complete_turn("First response");
    thread.start_turn("Second message");
    thread.complete_turn("Second response");

    let messages = thread.messages();
    assert_eq!(messages.len(), 4);
}

#[test]
fn test_restore_from_messages() {
    let mut thread = Thread::new(Uuid::new_v4());

    // First add some turns
    thread.start_turn("Original message");
    thread.complete_turn("Original response");

    // Now restore from different messages
    let messages = vec![
        ChatMessage::user("Hello"),
        ChatMessage::assistant("Hi there!"),
        ChatMessage::user("How are you?"),
        ChatMessage::assistant("I'm good!"),
    ];

    thread.restore_from_messages(messages);

    assert_eq!(thread.turns.len(), 2);
    assert_eq!(thread.turns[0].user_input, "Hello");
    assert_eq!(thread.turns[0].response, Some("Hi there!".to_string()));
    assert_eq!(thread.turns[1].user_input, "How are you?");
    assert_eq!(thread.turns[1].response, Some("I'm good!".to_string()));
    assert_eq!(thread.state, ThreadState::Idle);
}

#[test]
fn test_restore_from_messages_incomplete_turn() {
    let mut thread = Thread::new(Uuid::new_v4());

    // Messages with incomplete last turn (no assistant response)
    let messages = vec![
        ChatMessage::user("Hello"),
        ChatMessage::assistant("Hi there!"),
        ChatMessage::user("How are you?"),
    ];

    thread.restore_from_messages(messages);

    assert_eq!(thread.turns.len(), 2);
    assert_eq!(thread.turns[1].user_input, "How are you?");
    assert!(thread.turns[1].response.is_none());
}

#[test]
fn test_thread_with_id_restore_messages() {
    let thread_id = Uuid::new_v4();
    let session_id = Uuid::new_v4();
    let mut thread = Thread::with_id(thread_id, session_id);

    let messages = vec![
        ChatMessage::user("Hello from DB"),
        ChatMessage::assistant("Restored response"),
    ];
    thread.restore_from_messages(messages);

    assert_eq!(thread.id, thread_id);
    assert_eq!(thread.turns.len(), 1);
    assert_eq!(thread.turns[0].user_input, "Hello from DB");
    assert_eq!(
        thread.turns[0].response,
        Some("Restored response".to_string())
    );
}

#[test]
fn test_restore_from_messages_empty() {
    let mut thread = Thread::new(Uuid::new_v4());

    // Add a turn first, then restore with empty vec
    thread.start_turn("hello");
    thread.complete_turn("hi");
    assert_eq!(thread.turns.len(), 1);

    thread.restore_from_messages(Vec::new());

    // Should clear all turns and stay idle
    assert!(thread.turns.is_empty());
    assert_eq!(thread.state, ThreadState::Idle);
}

#[test]
fn test_restore_from_messages_only_assistant_messages() {
    let mut thread = Thread::new(Uuid::new_v4());

    // Only assistant messages (no user messages to anchor turns)
    let messages = vec![
        ChatMessage::assistant("I'm here"),
        ChatMessage::assistant("Still here"),
    ];

    thread.restore_from_messages(messages);

    // Assistant-only messages have no user turn to attach to, so
    // they should be skipped entirely.
    assert!(thread.turns.is_empty());
}

#[test]
fn test_restore_from_messages_multiple_user_messages_in_a_row() {
    let mut thread = Thread::new(Uuid::new_v4());

    // Two user messages with no assistant response between them
    let messages = vec![
        ChatMessage::user("first"),
        ChatMessage::user("second"),
        ChatMessage::assistant("reply to second"),
    ];

    thread.restore_from_messages(messages);

    // First user message becomes a turn with no response,
    // second user message pairs with the assistant response.
    assert_eq!(thread.turns.len(), 2);
    assert_eq!(thread.turns[0].user_input, "first");
    assert!(thread.turns[0].response.is_none());
    assert_eq!(thread.turns[1].user_input, "second");
    assert_eq!(
        thread.turns[1].response,
        Some("reply to second".to_string())
    );
}

#[test]
fn test_messages_with_incomplete_last_turn() {
    let mut thread = Thread::new(Uuid::new_v4());

    thread.start_turn("first");
    thread.complete_turn("first reply");
    thread.start_turn("second (in progress)");

    let messages = thread.messages();
    // Should have 3 messages: user, assistant, user (no assistant for in-progress)
    assert_eq!(messages.len(), 3);
    assert_eq!(messages[0].content, "first");
    assert_eq!(messages[1].content, "first reply");
    assert_eq!(messages[2].content, "second (in progress)");
}

// Regression tests for #568: tool call history must survive hydration.

#[test]
fn test_messages_includes_tool_calls() {
    let mut thread = Thread::new(Uuid::new_v4());

    thread.start_turn("Search for X");
    {
        let turn = thread.turns.last_mut().unwrap();
        turn.record_tool_call("memory_search", serde_json::json!({"query": "X"}));
        turn.record_tool_result(serde_json::json!("Found X in doc.md"));
    }
    thread.complete_turn("I found X in doc.md.");

    let messages = thread.messages();
    // user + assistant_with_tool_calls + tool_result + assistant = 4
    assert_eq!(messages.len(), 4);

    assert_eq!(messages[0].role, crate::llm::Role::User);
    assert_eq!(messages[0].content, "Search for X");

    assert_eq!(messages[1].role, crate::llm::Role::Assistant);
    assert!(messages[1].tool_calls.is_some());
    let tcs = messages[1].tool_calls.as_ref().unwrap();
    assert_eq!(tcs.len(), 1);
    assert_eq!(tcs[0].name, "memory_search");

    assert_eq!(messages[2].role, crate::llm::Role::Tool);
    assert!(messages[2].content.contains("Found X"));

    assert_eq!(messages[3].role, crate::llm::Role::Assistant);
    assert_eq!(messages[3].content, "I found X in doc.md.");
}

#[test]
fn test_messages_multiple_tool_calls_per_turn() {
    let mut thread = Thread::new(Uuid::new_v4());

    thread.start_turn("Do two things");
    {
        let turn = thread.turns.last_mut().unwrap();
        turn.record_tool_call("echo", serde_json::json!({"msg": "a"}));
        turn.record_tool_result(serde_json::json!("a"));
        turn.record_tool_call("time", serde_json::json!({}));
        turn.record_tool_error("timeout");
    }
    thread.complete_turn("Done.");

    let messages = thread.messages();
    // user + assistant_with_calls(2) + tool_result + tool_result + assistant = 5
    assert_eq!(messages.len(), 5);

    let tcs = messages[1].tool_calls.as_ref().unwrap();
    assert_eq!(tcs.len(), 2);

    // First tool: success
    assert_eq!(messages[2].content, "a");
    // Second tool: error (passed through directly, no wrapping)
    assert!(messages[3].content.contains("timeout"));
}

#[test]
fn test_restore_from_messages_with_tool_calls() {
    let mut thread = Thread::new(Uuid::new_v4());

    // Build a message sequence with tool calls
    let tc = ToolCall {
        id: "call_0".to_string(),
        name: "search".to_string(),
        arguments: serde_json::json!({"q": "test"}),
    };
    let messages = vec![
        ChatMessage::user("Find test"),
        ChatMessage::assistant_with_tool_calls(None, vec![tc]),
        ChatMessage::tool_result("call_0", "search", "result: found"),
        ChatMessage::assistant("Found it."),
    ];

    thread.restore_from_messages(messages);

    assert_eq!(thread.turns.len(), 1);
    let turn = &thread.turns[0];
    assert_eq!(turn.user_input, "Find test");
    assert_eq!(turn.tool_calls.len(), 1);
    assert_eq!(turn.tool_calls[0].name, "search");
    assert_eq!(
        turn.tool_calls[0].result,
        Some(serde_json::Value::String("result: found".to_string()))
    );
    assert_eq!(turn.response, Some("Found it.".to_string()));
}

#[test]
fn test_restore_from_messages_with_tool_error() {
    let mut thread = Thread::new(Uuid::new_v4());

    let tc = ToolCall {
        id: "call_0".to_string(),
        name: "http".to_string(),
        arguments: serde_json::json!({}),
    };
    let messages = vec![
        ChatMessage::user("Fetch URL"),
        ChatMessage::assistant_with_tool_calls(None, vec![tc]),
        ChatMessage::tool_result("call_0", "http", "Error: timeout"),
        ChatMessage::assistant("The request timed out."),
    ];

    thread.restore_from_messages(messages);

    // restore_from_messages stores all tool content as result (not error),
    // because it can't reliably distinguish errors from results that happen
    // to start with "Error: ". The content is preserved for LLM context.
    let turn = &thread.turns[0];
    assert_eq!(
        turn.tool_calls[0].result,
        Some(serde_json::Value::String("Error: timeout".to_string()))
    );
}

#[test]
fn test_messages_round_trip_with_tools() {
    // Build a thread with tool calls, get messages(), restore, get messages() again
    // The two message sequences should be equivalent.
    let mut thread = Thread::new(Uuid::new_v4());

    thread.start_turn("Do search");
    {
        let turn = thread.turns.last_mut().unwrap();
        turn.record_tool_call("search", serde_json::json!({"q": "test"}));
        turn.record_tool_result(serde_json::json!("found"));
    }
    thread.complete_turn("Here are results.");

    let messages_original = thread.messages();

    // Restore into a new thread
    let mut thread2 = Thread::new(Uuid::new_v4());
    thread2.restore_from_messages(messages_original.clone());

    let messages_restored = thread2.messages();

    // Same number of messages
    assert_eq!(messages_original.len(), messages_restored.len());

    // Same roles
    for (orig, rest) in messages_original.iter().zip(messages_restored.iter()) {
        assert_eq!(orig.role, rest.role);
    }

    // Same final response
    assert_eq!(
        messages_original.last().unwrap().content,
        messages_restored.last().unwrap().content
    );
}

#[test]
fn test_restore_multi_stage_tool_calls() {
    let mut thread = Thread::new(Uuid::new_v4());

    let tc1 = ToolCall {
        id: "call_a".to_string(),
        name: "search".to_string(),
        arguments: serde_json::json!({"q": "data"}),
    };
    let tc2 = ToolCall {
        id: "call_b".to_string(),
        name: "write".to_string(),
        arguments: serde_json::json!({"path": "out.txt"}),
    };
    let messages = vec![
        ChatMessage::user("Find and save"),
        ChatMessage::assistant_with_tool_calls(None, vec![tc1]),
        ChatMessage::tool_result("call_a", "search", "found data"),
        ChatMessage::assistant_with_tool_calls(None, vec![tc2]),
        ChatMessage::tool_result("call_b", "write", "written"),
        ChatMessage::assistant("Done, saved to out.txt"),
    ];

    thread.restore_from_messages(messages);

    assert_eq!(thread.turns.len(), 1);
    let turn = &thread.turns[0];
    assert_eq!(turn.tool_calls.len(), 2);
    assert_eq!(turn.tool_calls[0].name, "search");
    assert_eq!(turn.tool_calls[1].name, "write");
    assert_eq!(
        turn.tool_calls[0].result,
        Some(serde_json::Value::String("found data".to_string()))
    );
    assert_eq!(
        turn.tool_calls[1].result,
        Some(serde_json::Value::String("written".to_string()))
    );
    assert_eq!(turn.response, Some("Done, saved to out.txt".to_string()));
}

#[test]
fn test_messages_truncates_large_tool_results() {
    let mut thread = Thread::new(Uuid::new_v4());

    thread.start_turn("Read big file");
    {
        let turn = thread.turns.last_mut().unwrap();
        turn.record_tool_call("read_file", serde_json::json!({"path": "big.txt"}));
        let big_result = "x".repeat(2000);
        turn.record_tool_result(serde_json::json!(big_result));
    }
    thread.complete_turn("Here's the file content.");

    let messages = thread.messages();
    let tool_result_content = &messages[2].content;
    assert!(
        tool_result_content.len() <= 1010,
        "Tool result should be truncated, got {} chars",
        tool_result_content.len()
    );
    assert!(tool_result_content.ends_with("..."));
}
