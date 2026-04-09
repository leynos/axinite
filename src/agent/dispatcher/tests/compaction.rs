//! Message compaction tests.

use super::super::types::compact_messages_for_retry;
use super::*;

/// Asserts the standard preamble of a compacted message list:
/// - Expected total length
/// - First message is System with expected content
/// - Second message contains "compacted" (the compaction note)
fn assert_compact_preamble(compacted: &[ChatMessage], expected_len: usize, system_content: &str) {
    assert_eq!(compacted.len(), expected_len);
    assert_eq!(compacted[0].role, Role::System);
    assert_eq!(compacted[0].content, system_content);
    assert!(compacted[1].content.contains("compacted"));
}

/// Asserts a message has the expected role and content.
fn assert_message(msg: &ChatMessage, role: Role, content: &str) {
    assert_eq!(msg.role, role);
    assert_eq!(msg.content, content);
}

#[test]
fn test_compact_keeps_system_and_last_user_exchange() {
    let messages = vec![
        ChatMessage::system("You are a helpful assistant."),
        ChatMessage::user("First question"),
        ChatMessage::assistant("First answer"),
        ChatMessage::user("Second question"),
        ChatMessage::assistant("Second answer"),
        ChatMessage::user("Third question"),
        ChatMessage::assistant_with_tool_calls(
            None,
            vec![ToolCall {
                id: "call_1".to_string(),
                name: "echo".to_string(),
                arguments: serde_json::json!({"message": "hi"}),
            }],
        ),
        ChatMessage::tool_result("call_1", "echo", "hi"),
    ];

    let compacted = compact_messages_for_retry(&messages);

    // Should have: system prompt + compaction note + last user msg + tool call + tool result
    assert_compact_preamble(&compacted, 5, "You are a helpful assistant.");
    assert_message(&compacted[2], Role::User, "Third question");
    assert_eq!(compacted[3].role, Role::Assistant); // tool call
    assert_eq!(compacted[4].role, Role::Tool); // tool result
}

#[test]
fn test_compact_preserves_multiple_system_messages() {
    let messages = vec![
        ChatMessage::system("System prompt"),
        ChatMessage::system("Skill context"),
        ChatMessage::user("Old question"),
        ChatMessage::assistant("Old answer"),
        ChatMessage::system("Nudge message"),
        ChatMessage::user("Current question"),
    ];

    let compacted = compact_messages_for_retry(&messages);

    // 3 system messages + compaction note + last user message
    assert_eq!(compacted.len(), 5);
    assert_message(&compacted[0], Role::System, "System prompt");
    assert_message(&compacted[1], Role::System, "Skill context");
    assert_message(&compacted[2], Role::System, "Nudge message");
    assert!(compacted[3].content.contains("compacted")); // note
    assert_message(&compacted[4], Role::User, "Current question");
}

#[test]
fn test_compact_single_user_message_keeps_everything() {
    let messages = vec![
        ChatMessage::system("System prompt"),
        ChatMessage::user("Only question"),
    ];

    let compacted = compact_messages_for_retry(&messages);

    // system + compaction note + user
    assert_eq!(compacted.len(), 3);
    assert_eq!(compacted[0].content, "System prompt");
    assert!(compacted[1].content.contains("compacted"));
    assert_eq!(compacted[2].content, "Only question");
}

#[test]
fn test_compact_no_user_messages_keeps_non_system() {
    let messages = vec![
        ChatMessage::system("System prompt"),
        ChatMessage::assistant("Stray assistant message"),
    ];

    let compacted = compact_messages_for_retry(&messages);

    // system + assistant (no user message found, keeps all non-system)
    assert_eq!(compacted.len(), 2);
    assert_eq!(compacted[0].role, Role::System);
    assert_eq!(compacted[1].role, Role::Assistant);
}

#[test]
fn test_compact_drops_old_history_but_keeps_current_turn_tools() {
    // Simulate a multi-turn conversation where the current turn has
    // multiple tool calls and results.
    let messages = vec![
        ChatMessage::system("System prompt"),
        ChatMessage::user("Question 1"),
        ChatMessage::assistant("Answer 1"),
        ChatMessage::user("Question 2"),
        ChatMessage::assistant("Answer 2"),
        ChatMessage::user("Question 3"),
        ChatMessage::assistant("Answer 3"),
        ChatMessage::user("Current question"),
        ChatMessage::assistant_with_tool_calls(
            None,
            vec![
                ToolCall {
                    id: "c1".to_string(),
                    name: "http".to_string(),
                    arguments: serde_json::json!({}),
                },
                ToolCall {
                    id: "c2".to_string(),
                    name: "echo".to_string(),
                    arguments: serde_json::json!({}),
                },
            ],
        ),
        ChatMessage::tool_result("c1", "http", "response data"),
        ChatMessage::tool_result("c2", "echo", "echoed"),
    ];

    let compacted = compact_messages_for_retry(&messages);

    // system + note + user + assistant(tool_calls) + tool_result + tool_result
    assert_compact_preamble(&compacted, 6, "System prompt");
    assert_message(&compacted[2], Role::User, "Current question");
    assert!(compacted[3].tool_calls.is_some()); // assistant with tool calls
    assert_eq!(compacted[4].name.as_deref(), Some("http"));
    assert_eq!(compacted[5].name.as_deref(), Some("echo"));
}

#[test]
fn test_compact_no_duplicate_system_after_last_user() {
    // A system nudge message injected AFTER the last user message must
    // not be duplicated — it should only appear once (via extend_from_slice).
    let messages = vec![
        ChatMessage::system("System prompt"),
        ChatMessage::user("Question"),
        ChatMessage::system("Nudge: wrap up"),
        ChatMessage::assistant_with_tool_calls(
            None,
            vec![ToolCall {
                id: "c1".to_string(),
                name: "echo".to_string(),
                arguments: serde_json::json!({}),
            }],
        ),
        ChatMessage::tool_result("c1", "echo", "done"),
    ];

    let compacted = compact_messages_for_retry(&messages);

    // system prompt + note + user + nudge + assistant + tool_result = 6
    assert_compact_preamble(&compacted, 6, "System prompt");
    assert_message(&compacted[2], Role::User, "Question");
    assert_message(&compacted[3], Role::System, "Nudge: wrap up"); // not duplicated
    assert_eq!(compacted[4].role, Role::Assistant);
    assert_eq!(compacted[5].role, Role::Tool);

    // Verify "Nudge: wrap up" appears exactly once
    let nudge_count = compacted
        .iter()
        .filter(|m| m.content == "Nudge: wrap up")
        .count();
    assert_eq!(nudge_count, 1);
}

// === QA Plan P2 - 2.7: Context length recovery ===

#[tokio::test]
async fn test_context_length_recovery_via_compaction_and_retry() {
    // Simulates the dispatcher's recovery path:
    //   1. Provider returns ContextLengthExceeded
    //   2. compact_messages_for_retry reduces context
    //   3. Retry with compacted messages succeeds
    use crate::llm::Reasoning;
    use crate::testing::StubLlm;

    let stub = Arc::new(StubLlm::failing_non_transient("ctx-bomb"));

    let reasoning = Reasoning::new(stub.clone());

    // Build a fat context with lots of history.
    let messages = vec![
        ChatMessage::system("You are a helpful assistant."),
        ChatMessage::user("First question"),
        ChatMessage::assistant("First answer"),
        ChatMessage::user("Second question"),
        ChatMessage::assistant("Second answer"),
        ChatMessage::user("Third question"),
        ChatMessage::assistant("Third answer"),
        ChatMessage::user("Current request"),
    ];

    let context = crate::llm::ReasoningContext::new().with_messages(messages.clone());

    // Step 1: First call fails with ContextLengthExceeded.
    let err = reasoning.respond_with_tools(&context).await.unwrap_err();
    assert!(
        matches!(err, crate::error::LlmError::ContextLengthExceeded { .. }),
        "Expected ContextLengthExceeded, got: {:?}",
        err
    );
    assert_eq!(stub.calls(), 1);

    // Step 2: Compact messages (same as dispatcher lines 226).
    let compacted = compact_messages_for_retry(&messages);
    // Should have dropped the old history, kept system + note + last user.
    assert!(compacted.len() < messages.len());
    assert_eq!(compacted.last().unwrap().content, "Current request");

    // Step 3: Switch provider to success and retry.
    stub.set_failing(false);
    let retry_context = crate::llm::ReasoningContext::new().with_messages(compacted);

    let result = reasoning.respond_with_tools(&retry_context).await;
    assert!(result.is_ok(), "Retry after compaction should succeed");
    assert_eq!(stub.calls(), 2);
}
