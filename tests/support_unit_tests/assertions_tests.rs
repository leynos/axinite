//! Unit tests for the shared assertion helpers in
//! [`crate::support::assertions`].
//!
//! These tests cover case-insensitive substring matching, per-tool result
//! filtering, and panic-message diagnostics captured with `catch_unwind`.
//! They also exercise edge-case guards such as empty
//! `expected_substrings` inputs and the `TraceExpects`-driven checks in
//! `verify_expects`.

use std::any::Any;
use std::collections::HashMap;
use std::panic::{AssertUnwindSafe, UnwindSafe, catch_unwind};

use crate::support::assertions::*;
use crate::support::trace_llm::TraceExpects;

fn panic_message(payload: Box<dyn Any + Send>) -> String {
    match payload.downcast::<String>() {
        Ok(message) => *message,
        Err(payload) => match payload.downcast::<&'static str>() {
            Ok(message) => (*message).to_string(),
            Err(_) => "non-string panic payload".to_string(),
        },
    }
}

fn capture_panic_message<F>(f: F) -> String
where
    F: FnOnce() + UnwindSafe,
{
    let panic = catch_unwind(AssertUnwindSafe(f)).expect_err("helper should panic");
    panic_message(panic)
}

#[test]
fn all_tools_succeeded_passes_when_all_true() {
    let completed = vec![("echo".to_string(), true), ("time".to_string(), true)];
    assert_all_tools_succeeded(&completed);
}

#[test]
fn all_tools_succeeded_passes_on_empty() {
    assert_all_tools_succeeded(&[]);
}

#[test]
#[should_panic(expected = "Expected all tools to succeed")]
fn all_tools_succeeded_panics_on_failure() {
    let completed = vec![("echo".to_string(), true), ("shell".to_string(), false)];
    assert_all_tools_succeeded(&completed);
}

#[test]
fn tool_succeeded_passes_when_present_and_true() {
    let completed = vec![("echo".to_string(), true), ("time".to_string(), false)];
    assert_tool_succeeded(&completed, "echo");
}

#[test]
#[should_panic(expected = "Expected 'echo' to complete successfully")]
fn tool_succeeded_panics_when_tool_missing() {
    let completed = vec![("time".to_string(), true)];
    assert_tool_succeeded(&completed, "echo");
}

#[test]
#[should_panic(expected = "Expected 'shell' to complete successfully")]
fn tool_succeeded_panics_when_tool_failed() {
    let completed = vec![("shell".to_string(), false)];
    assert_tool_succeeded(&completed, "shell");
}

#[test]
fn tool_order_passes_for_correct_order() {
    let started: Vec<String> = vec!["write_file", "echo", "read_file"]
        .into_iter()
        .map(String::from)
        .collect();
    assert_tool_order(&started, &["write_file", "read_file"]);
}

#[test]
fn tool_order_passes_for_consecutive() {
    let started: Vec<String> = vec!["write_file", "read_file"]
        .into_iter()
        .map(String::from)
        .collect();
    assert_tool_order(&started, &["write_file", "read_file"]);
}

#[test]
#[should_panic(expected = "assert_tool_order")]
fn tool_order_panics_for_wrong_order() {
    let started: Vec<String> = vec!["read_file", "write_file"]
        .into_iter()
        .map(String::from)
        .collect();
    assert_tool_order(&started, &["write_file", "read_file"]);
}

#[test]
#[should_panic(expected = "assert_tool_order")]
fn tool_order_panics_for_missing_tool() {
    let started: Vec<String> = vec!["echo".to_string()];
    assert_tool_order(&started, &["echo", "write_file"]);
}

#[test]
fn tool_result_contains_matches_case_insensitively() {
    let results = vec![
        ("memory_search".to_string(), "irrelevant".to_string()),
        ("memory_tree".to_string(), "Alpha/Beta".to_string()),
    ];

    assert_tool_result_contains(&results, "memory_tree", &["alpha", "gamma"]);
}

#[test]
fn tool_result_contains_ignores_other_tools_when_matching() {
    let results = vec![
        ("memory_search".to_string(), "Alpha/Beta".to_string()),
        ("memory_tree".to_string(), "Gamma/Delta".to_string()),
    ];

    let panic = capture_panic_message(|| {
        assert_tool_result_contains(&results, "memory_tree", &["alpha"]);
    });

    assert!(
        panic.contains("No result for 'memory_tree' contained any of [\"alpha\"]"),
        "unexpected panic message"
    );
}

#[test]
fn tool_result_contains_panics_when_tool_is_missing() {
    let results = vec![("memory_search".to_string(), "Alpha/Beta".to_string())];

    let panic = capture_panic_message(|| {
        assert_tool_result_contains(&results, "memory_tree", &["alpha"]);
    });

    assert_eq!(panic, "Expected at least one result for tool 'memory_tree'");
}

#[test]
fn tool_result_contains_rejects_empty_expected_substrings() {
    let results = vec![("memory_tree".to_string(), "Alpha/Beta".to_string())];

    let panic = capture_panic_message(|| {
        assert_tool_result_contains(&results, "memory_tree", &[]);
    });

    assert_eq!(
        panic,
        "expected_substrings must not be empty when asserting tool results for 'memory_tree'"
    );
}

#[test]
fn tool_result_contains_rejects_whitespace_only_expected_substrings() {
    let results = vec![("memory_tree".to_string(), "Alpha/Beta".to_string())];

    let panic = capture_panic_message(|| {
        assert_tool_result_contains(&results, "memory_tree", &["   "]);
    });

    assert_eq!(
        panic,
        "expected_substrings entries must be non-empty when asserting tool results for 'memory_tree'"
    );
}

#[test]
fn verify_expects_reports_missing_tool_result_with_label() {
    let expects = TraceExpects {
        tool_results_contain: HashMap::from([("memory_tree".to_string(), "alpha".to_string())]),
        ..TraceExpects::default()
    };

    let panic = capture_panic_message(|| {
        verify_expects(&expects, &[], &[], &[], &[], "turn 0");
    });

    assert_eq!(
        panic,
        "[turn 0] tool_results_contain: no result for tool \"memory_tree\", got: []"
    );
}

#[test]
fn verify_expects_reports_missing_substring_with_preview() {
    let expects = TraceExpects {
        tool_results_contain: HashMap::from([("memory_tree".to_string(), "alpha".to_string())]),
        ..TraceExpects::default()
    };
    let results = vec![("memory_tree".to_string(), "Gamma/Delta".to_string())];

    let panic = capture_panic_message(|| {
        verify_expects(&expects, &[], &[], &[], &results, "turn 0");
    });

    assert_eq!(
        panic,
        "[turn 0] tool_results_contain: tool \"memory_tree\" result does not contain \"alpha\", got: \"Gamma/Delta\""
    );
}
