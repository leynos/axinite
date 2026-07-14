//! Unit tests for action records and context memory tracking.

use super::*;

#[test]
fn test_action_record() {
    let action = ActionRecord::new(0, "test", serde_json::json!({"key": "value"}));
    assert_eq!(action.sequence, 0);
    assert!(!action.success);

    let action = action.succeed(
        Some("raw".to_string()),
        serde_json::json!({"result": "ok"}),
        Duration::from_millis(100),
    );
    assert!(action.success);
}

#[test]
fn test_conversation_memory() {
    let mut memory = ConversationMemory::new(3);
    memory.add(ChatMessage::user("Hello"));
    memory.add(ChatMessage::assistant("Hi"));
    memory.add(ChatMessage::user("How are you?"));
    memory.add(ChatMessage::assistant("Good!"));

    assert_eq!(memory.len(), 3); // Oldest removed
}

#[test]
fn test_memory_totals() {
    let mut memory = Memory::new(Uuid::new_v4());

    let action1 = memory
        .create_action("tool1", serde_json::json!({}))
        .succeed(None, serde_json::json!({}), Duration::from_secs(1))
        .with_cost(Decimal::new(10, 1));
    memory.record_action(action1);

    let action2 = memory
        .create_action("tool2", serde_json::json!({}))
        .succeed(None, serde_json::json!({}), Duration::from_secs(2))
        .with_cost(Decimal::new(20, 1));
    memory.record_action(action2);

    assert_eq!(memory.total_cost(), Decimal::new(30, 1));
    assert_eq!(memory.total_duration(), Duration::from_secs(3));
    assert_eq!(memory.successful_actions(), 2);
}

#[test]
fn test_action_record_fail() {
    let action = ActionRecord::new(1, "broken_tool", serde_json::json!({"x": 1}));
    let action = action.fail("something went wrong", Duration::from_millis(50));

    assert!(!action.success);
    assert_eq!(action.error.as_deref(), Some("something went wrong"));
    assert_eq!(action.duration, Duration::from_millis(50));
    assert!(action.output_raw.is_none());
    assert!(action.output_sanitized.is_none());
}

#[test]
fn test_action_record_with_warnings() {
    let action = ActionRecord::new(0, "risky_tool", serde_json::json!({}));
    let action = action.with_warnings(vec!["suspicious pattern".into(), "possible xss".into()]);

    assert_eq!(action.sanitization_warnings.len(), 2);
    assert_eq!(action.sanitization_warnings[0], "suspicious pattern");
    assert_eq!(action.sanitization_warnings[1], "possible xss");
}

#[test]
fn test_action_record_with_cost() {
    let action = ActionRecord::new(0, "expensive_tool", serde_json::json!({}));
    let cost = Decimal::new(42, 2); // 0.42
    let action = action.with_cost(cost);

    assert_eq!(action.cost, Some(Decimal::new(42, 2)));
}

#[test]
fn test_action_record_new_defaults() {
    let action = ActionRecord::new(5, "my_tool", serde_json::json!({"key": "val"}));

    assert_eq!(action.sequence, 5);
    assert_eq!(action.tool_name, "my_tool");
    assert_eq!(action.input, serde_json::json!({"key": "val"}));
    assert!(!action.success);
    assert!(action.output_raw.is_none());
    assert!(action.output_sanitized.is_none());
    assert!(action.sanitization_warnings.is_empty());
    assert!(action.cost.is_none());
    assert_eq!(action.duration, Duration::ZERO);
    assert!(action.error.is_none());
}

#[test]
fn test_action_record_succeed_sets_fields() {
    let action = ActionRecord::new(0, "tool", serde_json::json!({}));
    let action = action.succeed(
        Some("raw output here".into()),
        serde_json::json!({"clean": true}),
        Duration::from_secs(7),
    );

    assert!(action.success);
    assert_eq!(action.output_raw.as_deref(), Some("raw output here"));
    assert_eq!(
        action.output_sanitized,
        Some(serde_json::json!({"clean": true}))
    );
    assert_eq!(action.duration, Duration::from_secs(7));
}

#[test]
fn test_conversation_memory_clear() {
    let mut mem = ConversationMemory::new(10);
    mem.add(ChatMessage::user("hello"));
    mem.add(ChatMessage::assistant("hi"));
    assert_eq!(mem.len(), 2);
    assert!(!mem.is_empty());

    mem.clear();
    assert_eq!(mem.len(), 0);
    assert!(mem.is_empty());
    assert!(mem.messages().is_empty());
}

#[test]
fn test_conversation_memory_last_n() {
    let mut mem = ConversationMemory::new(10);
    mem.add(ChatMessage::user("one"));
    mem.add(ChatMessage::assistant("two"));
    mem.add(ChatMessage::user("three"));
    mem.add(ChatMessage::assistant("four"));

    let last_2 = mem.last_n(2);
    assert_eq!(last_2.len(), 2);
    assert_eq!(last_2[0].content, "three");
    assert_eq!(last_2[1].content, "four");

    // Requesting more than available returns all
    let last_100 = mem.last_n(100);
    assert_eq!(last_100.len(), 4);
}

#[test]
fn test_conversation_memory_last_n_empty() {
    let mem = ConversationMemory::new(10);
    let result = mem.last_n(5);
    assert!(result.is_empty());
}

#[test]
fn test_conversation_memory_preserves_system_message_on_trim() {
    let mut mem = ConversationMemory::new(3);
    mem.add(ChatMessage::system("You are helpful"));
    mem.add(ChatMessage::user("msg1"));
    mem.add(ChatMessage::user("msg2"));

    // At capacity (3). Adding one more should trim, but keep system.
    mem.add(ChatMessage::user("msg3"));

    assert_eq!(mem.len(), 3);
    // System message must survive
    assert_eq!(mem.messages()[0].role, crate::llm::Role::System);
    assert_eq!(mem.messages()[0].content, "You are helpful");
    // Oldest non-system message (msg1) should be gone
    assert_eq!(mem.messages()[1].content, "msg2");
    assert_eq!(mem.messages()[2].content, "msg3");
}

#[test]
fn test_conversation_memory_trims_non_system_first() {
    let mut mem = ConversationMemory::new(2);
    mem.add(ChatMessage::system("sys"));
    mem.add(ChatMessage::user("a"));
    // Now at capacity. Add another.
    mem.add(ChatMessage::user("b"));

    assert_eq!(mem.len(), 2);
    assert_eq!(mem.messages()[0].role, crate::llm::Role::System);
    assert_eq!(mem.messages()[1].content, "b");
}

#[test]
fn test_conversation_memory_max_one_with_system_does_not_loop() {
    // Edge case: max_messages = 1 and only a system message.
    // Adding another message would try to trim but should not
    // remove the system message and get stuck.
    let mut mem = ConversationMemory::new(1);
    mem.add(ChatMessage::system("sys"));
    // The system message is already at capacity. Adding another
    // cannot trim the system message, so we end up with 2 (graceful).
    // The important thing is we don't infinite-loop.
    mem.add(ChatMessage::user("hello"));
    // Should have broken out rather than looping forever.
    // The system message is protected, so len may exceed max.
    assert!(mem.len() <= 2);
}

#[test]
fn test_memory_failed_actions() {
    let mut memory = Memory::new(Uuid::new_v4());

    let ok = memory.create_action("good", serde_json::json!({})).succeed(
        None,
        serde_json::json!({}),
        Duration::from_millis(1),
    );
    memory.record_action(ok);

    let err = memory
        .create_action("bad", serde_json::json!({}))
        .fail("oops", Duration::from_millis(2));
    memory.record_action(err);

    assert_eq!(memory.successful_actions(), 1);
    assert_eq!(memory.failed_actions(), 1);
}

#[test]
fn test_memory_last_action() {
    let mut memory = Memory::new(Uuid::new_v4());
    assert!(memory.last_action().is_none());

    let a1 = memory
        .create_action("first", serde_json::json!({}))
        .succeed(None, serde_json::json!({}), Duration::ZERO);
    memory.record_action(a1);

    let a2 = memory
        .create_action("second", serde_json::json!({}))
        .fail("nope", Duration::ZERO);
    memory.record_action(a2);

    let last = memory.last_action().unwrap();
    assert_eq!(last.tool_name, "second");
}

#[test]
fn test_memory_actions_by_tool() {
    let mut memory = Memory::new(Uuid::new_v4());

    for _ in 0..3 {
        let a = memory
            .create_action("shell", serde_json::json!({}))
            .succeed(None, serde_json::json!({}), Duration::ZERO);
        memory.record_action(a);
    }
    let a = memory.create_action("http", serde_json::json!({})).succeed(
        None,
        serde_json::json!({}),
        Duration::ZERO,
    );
    memory.record_action(a);

    assert_eq!(memory.actions_by_tool("shell").len(), 3);
    assert_eq!(memory.actions_by_tool("http").len(), 1);
    assert_eq!(memory.actions_by_tool("nonexistent").len(), 0);
}

#[test]
fn test_memory_create_action_increments_sequence() {
    let mut memory = Memory::new(Uuid::new_v4());

    let a0 = memory.create_action("t", serde_json::json!({}));
    assert_eq!(a0.sequence, 0);

    let a1 = memory.create_action("t", serde_json::json!({}));
    assert_eq!(a1.sequence, 1);

    let a2 = memory.create_action("t", serde_json::json!({}));
    assert_eq!(a2.sequence, 2);
}

#[test]
fn test_memory_add_message_delegates_to_conversation() {
    let mut memory = Memory::new(Uuid::new_v4());
    assert!(memory.conversation.is_empty());

    memory.add_message(ChatMessage::user("hello"));
    memory.add_message(ChatMessage::assistant("hi"));

    assert_eq!(memory.conversation.len(), 2);
    assert_eq!(memory.conversation.messages()[0].content, "hello");
}

#[test]
fn test_memory_total_cost_with_no_cost_actions() {
    let mut memory = Memory::new(Uuid::new_v4());

    // Actions without cost should contribute zero
    let a = memory
        .create_action("free_tool", serde_json::json!({}))
        .succeed(None, serde_json::json!({}), Duration::ZERO);
    memory.record_action(a);

    assert_eq!(memory.total_cost(), Decimal::ZERO);
}

#[test]
fn test_memory_total_duration_mixed() {
    let mut memory = Memory::new(Uuid::new_v4());

    let a1 = memory.create_action("t1", serde_json::json!({})).succeed(
        None,
        serde_json::json!({}),
        Duration::from_millis(100),
    );
    memory.record_action(a1);

    let a2 = memory
        .create_action("t2", serde_json::json!({}))
        .fail("err", Duration::from_millis(200));
    memory.record_action(a2);

    // Both successful and failed actions contribute to total duration
    assert_eq!(memory.total_duration(), Duration::from_millis(300));
}
