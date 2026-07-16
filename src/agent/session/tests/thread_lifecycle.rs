//! Tests for thread lifecycle: turns, interrupt/resume, failure handling,
//! truncation, pending approval/auth, and thread serialization.

use uuid::Uuid;

use crate::agent::session::{PendingApproval, Thread, ThreadState, Turn, TurnState};
use crate::llm::ChatMessage;

#[test]
fn test_thread_turns() {
    let mut thread = Thread::new(Uuid::new_v4());

    thread.start_turn("Hello");
    assert_eq!(thread.state, ThreadState::Processing);
    assert_eq!(thread.turns.len(), 1);

    thread.complete_turn("Hi there!");
    assert_eq!(thread.state, ThreadState::Idle);
    assert_eq!(thread.turns[0].response, Some("Hi there!".to_string()));
}

#[test]
fn test_turn_tool_calls() {
    let mut turn = Turn::new(0, "Test input");
    turn.record_tool_call("echo", serde_json::json!({"message": "test"}));
    turn.record_tool_result(serde_json::json!("test"));

    assert_eq!(turn.tool_calls.len(), 1);
    assert!(turn.tool_calls[0].result.is_some());
}

#[test]
fn test_enter_auth_mode() {
    let mut thread = Thread::new(Uuid::new_v4());
    assert!(thread.pending_auth.is_none());

    thread.enter_auth_mode("telegram".to_string());
    assert!(thread.pending_auth.is_some());
    assert_eq!(
        thread.pending_auth.as_ref().unwrap().extension_name,
        "telegram"
    );
}

#[test]
fn test_take_pending_auth() {
    let mut thread = Thread::new(Uuid::new_v4());
    thread.enter_auth_mode("notion".to_string());

    let pending = thread.take_pending_auth();
    assert!(pending.is_some());
    assert_eq!(pending.unwrap().extension_name, "notion");

    // Should be cleared after take
    assert!(thread.pending_auth.is_none());
    assert!(thread.take_pending_auth().is_none());
}

#[test]
fn test_pending_auth_serialization() {
    let mut thread = Thread::new(Uuid::new_v4());
    thread.enter_auth_mode("openai".to_string());

    let json = serde_json::to_string(&thread).expect("should serialize");
    assert!(json.contains("pending_auth"));
    assert!(json.contains("openai"));

    let restored: Thread = serde_json::from_str(&json).expect("should deserialize");
    assert!(restored.pending_auth.is_some());
    assert_eq!(restored.pending_auth.unwrap().extension_name, "openai");
}

#[test]
fn test_pending_auth_default_none() {
    // Deserialization of old data without pending_auth should default to None
    let mut thread = Thread::new(Uuid::new_v4());
    thread.pending_auth = None;
    let json = serde_json::to_string(&thread).expect("serialize");

    // Remove the pending_auth field to simulate old data
    let json = json.replace(",\"pending_auth\":null", "");
    let restored: Thread = serde_json::from_str(&json).expect("should deserialize");
    assert!(restored.pending_auth.is_none());
}

#[test]
fn test_in_flight_auth_is_transient_across_serde() {
    let mut thread = Thread::new(Uuid::new_v4());
    thread.in_flight_auth = true;

    let json = serde_json::to_string(&thread).expect("thread should serialize");
    assert!(
        !json.contains("in_flight_auth"),
        "in_flight_auth must be omitted from serialized JSON"
    );

    let restored: Thread = serde_json::from_str(&json).expect("thread should deserialize");
    assert!(
        !restored.in_flight_auth,
        "in_flight_auth must default to false after deserialization"
    );
}

#[test]
fn test_thread_with_id() {
    let specific_id = Uuid::new_v4();
    let session_id = Uuid::new_v4();
    let thread = Thread::with_id(specific_id, session_id);

    assert_eq!(thread.id, specific_id);
    assert_eq!(thread.session_id, session_id);
    assert_eq!(thread.state, ThreadState::Idle);
    assert!(thread.turns.is_empty());
}

#[test]
fn test_truncate_turns() {
    let mut thread = Thread::new(Uuid::new_v4());

    for i in 0..5 {
        thread.start_turn(format!("msg-{}", i));
        thread.complete_turn(format!("resp-{}", i));
    }
    assert_eq!(thread.turns.len(), 5);

    thread.truncate_turns(3);
    assert_eq!(thread.turns.len(), 3);

    // Should keep the most recent turns
    assert_eq!(thread.turns[0].user_input, "msg-2");
    assert_eq!(thread.turns[1].user_input, "msg-3");
    assert_eq!(thread.turns[2].user_input, "msg-4");

    // Turn numbers should be re-indexed
    assert_eq!(thread.turns[0].turn_number, 0);
    assert_eq!(thread.turns[1].turn_number, 1);
    assert_eq!(thread.turns[2].turn_number, 2);
}

#[test]
fn test_truncate_turns_noop_when_fewer() {
    let mut thread = Thread::new(Uuid::new_v4());

    thread.start_turn("only one");
    thread.complete_turn("response");

    thread.truncate_turns(10);
    assert_eq!(thread.turns.len(), 1);
    assert_eq!(thread.turns[0].user_input, "only one");
}

#[test]
fn test_thread_interrupt_and_resume() {
    let mut thread = Thread::new(Uuid::new_v4());

    thread.start_turn("do something");
    assert_eq!(thread.state, ThreadState::Processing);

    thread.interrupt();
    assert_eq!(thread.state, ThreadState::Interrupted);

    let last_turn = thread.last_turn().unwrap();
    assert_eq!(last_turn.state, TurnState::Interrupted);
    assert!(last_turn.completed_at.is_some());

    thread.resume();
    assert_eq!(thread.state, ThreadState::Idle);
}

#[test]
fn test_resume_only_from_interrupted() {
    let mut thread = Thread::new(Uuid::new_v4());

    // Idle thread: resume should be a no-op
    assert_eq!(thread.state, ThreadState::Idle);
    thread.resume();
    assert_eq!(thread.state, ThreadState::Idle);

    // Processing thread: resume should not change state
    thread.start_turn("work");
    assert_eq!(thread.state, ThreadState::Processing);
    thread.resume();
    assert_eq!(thread.state, ThreadState::Processing);
}

#[test]
fn test_turn_fail() {
    let mut thread = Thread::new(Uuid::new_v4());

    thread.start_turn("risky operation");
    thread.fail_turn("connection timed out");

    assert_eq!(thread.state, ThreadState::Idle);

    let turn = thread.last_turn().unwrap();
    assert_eq!(turn.state, TurnState::Failed);
    assert_eq!(turn.error, Some("connection timed out".to_string()));
    assert!(turn.response.is_none());
    assert!(turn.completed_at.is_some());
}

#[test]
fn test_thread_serialization_round_trip() {
    let mut thread = Thread::new(Uuid::new_v4());

    thread.start_turn("hello");
    thread.complete_turn("world");

    let json = serde_json::to_string(&thread).unwrap();
    let restored: Thread = serde_json::from_str(&json).unwrap();

    assert_eq!(restored.id, thread.id);
    assert_eq!(restored.session_id, thread.session_id);
    assert_eq!(restored.turns.len(), 1);
    assert_eq!(restored.turns[0].user_input, "hello");
    assert_eq!(restored.turns[0].response, Some("world".to_string()));
}

#[test]
fn test_turn_tool_call_error() {
    let mut turn = Turn::new(0, "test");
    turn.record_tool_call("http", serde_json::json!({"url": "example.com"}));
    turn.record_tool_error("timeout");

    assert_eq!(turn.tool_calls.len(), 1);
    assert_eq!(turn.tool_calls[0].error, Some("timeout".to_string()));
    assert!(turn.tool_calls[0].result.is_none());
}

#[test]
fn test_turn_number_increments() {
    let mut thread = Thread::new(Uuid::new_v4());

    // Before any turns, turn_number() is 1 (1-indexed for display)
    assert_eq!(thread.turn_number(), 1);

    thread.start_turn("first");
    thread.complete_turn("done");
    assert_eq!(thread.turn_number(), 2);

    thread.start_turn("second");
    assert_eq!(thread.turn_number(), 3);
}

#[test]
fn test_complete_turn_on_empty_thread() {
    let mut thread = Thread::new(Uuid::new_v4());

    // Completing a turn when there are no turns should be a safe no-op
    thread.complete_turn("phantom response");
    assert_eq!(thread.state, ThreadState::Idle);
    assert!(thread.turns.is_empty());
}

#[test]
fn test_fail_turn_on_empty_thread() {
    let mut thread = Thread::new(Uuid::new_v4());

    // Failing a turn when there are no turns should be a safe no-op
    thread.fail_turn("phantom error");
    assert_eq!(thread.state, ThreadState::Idle);
    assert!(thread.turns.is_empty());
}

#[test]
fn test_pending_approval_flow() {
    let mut thread = Thread::new(Uuid::new_v4());

    let approval = PendingApproval {
        request_id: Uuid::new_v4(),
        tool_name: "shell".to_string(),
        parameters: serde_json::json!({"command": "rm -rf /"}),
        display_parameters: serde_json::json!({"command": "rm -rf /"}),
        description: "dangerous command".to_string(),
        tool_call_id: "call_123".to_string(),
        context_messages: vec![ChatMessage::user("do it")],
        deferred_tool_calls: vec![],
        user_timezone: None,
    };

    thread.await_approval(approval);
    assert_eq!(thread.state, ThreadState::AwaitingApproval);
    assert!(thread.pending_approval.is_some());

    let taken = thread.take_pending_approval();
    assert!(taken.is_some());
    assert_eq!(taken.unwrap().tool_name, "shell");
    assert!(thread.pending_approval.is_none());
}

#[test]
fn test_clear_pending_approval() {
    let mut thread = Thread::new(Uuid::new_v4());

    let approval = PendingApproval {
        request_id: Uuid::new_v4(),
        tool_name: "http".to_string(),
        parameters: serde_json::json!({}),
        display_parameters: serde_json::json!({}),
        description: "test".to_string(),
        tool_call_id: "call_456".to_string(),
        context_messages: vec![],
        deferred_tool_calls: vec![],
        user_timezone: None,
    };

    thread.await_approval(approval);
    thread.clear_pending_approval();

    assert_eq!(thread.state, ThreadState::Idle);
    assert!(thread.pending_approval.is_none());
}
