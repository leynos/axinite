//! Tests for turn formatting and internal compaction result types.

use uuid::Uuid;

use crate::agent::session::Thread;

use super::super::{CompactionPartial, format_turns_for_storage};

#[test]
fn test_format_turns() {
    let mut thread = Thread::new(Uuid::new_v4());
    thread.start_turn("Hello");
    thread.complete_turn("Hi there");
    thread.start_turn("How are you?");
    thread.complete_turn("I'm good!");

    let formatted = format_turns_for_storage(&thread.turns);
    assert!(formatted.contains("Turn 1"));
    assert!(formatted.contains("Hello"));
    assert!(formatted.contains("Turn 2"));
}

#[test]
fn test_compaction_partial_empty() {
    let partial = CompactionPartial::empty();
    assert_eq!(partial.turns_removed, 0);
    assert!(!partial.summary_written);
}

// ------------------------------------------------------------------
// format_turns_for_storage includes tool calls
// ------------------------------------------------------------------

#[test]
fn test_format_turns_for_storage_with_tool_calls() {
    let mut thread = Thread::new(Uuid::new_v4());
    thread.start_turn("Search for X");
    // Record a tool call on the current turn
    if let Some(turn) = thread.turns.last_mut() {
        turn.record_tool_call("search", serde_json::json!({"query": "X"}));
    }
    thread.complete_turn("Found X");

    let formatted = format_turns_for_storage(&thread.turns);
    assert!(formatted.contains("Turn 1"));
    assert!(formatted.contains("Search for X"));
    assert!(formatted.contains("Found X"));
    assert!(formatted.contains("Tools: search"));
}

// ------------------------------------------------------------------
// format_turns_for_storage with no response (incomplete turn)
// ------------------------------------------------------------------

#[test]
fn test_format_turns_for_storage_incomplete_turn() {
    let mut thread = Thread::new(Uuid::new_v4());
    thread.start_turn("In progress message");
    // Don't complete the turn

    let formatted = format_turns_for_storage(&thread.turns);
    assert!(formatted.contains("Turn 1"));
    assert!(formatted.contains("In progress message"));
    // No "Agent:" line since response is None
    assert!(!formatted.contains("Agent:"));
}

// ------------------------------------------------------------------
// format_turns_for_storage empty list
// ------------------------------------------------------------------

#[test]
fn test_format_turns_for_storage_empty() {
    let formatted = format_turns_for_storage(&[]);
    assert!(formatted.is_empty());
}
