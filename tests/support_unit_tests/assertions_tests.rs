//! Assertions helper tests.

use std::any::Any;
use std::collections::HashMap;
use std::panic::{AssertUnwindSafe, catch_unwind};

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

    let panic = catch_unwind(AssertUnwindSafe(|| {
        assert_tool_result_contains(&results, "memory_tree", &["alpha"]);
    }))
    .expect_err("helper should fail when only another tool matches");

    assert!(
        panic_message(panic).contains("No result for 'memory_tree' contained any of [\"alpha\"]"),
        "unexpected panic message"
    );
}

#[test]
fn tool_result_contains_panics_when_tool_is_missing() {
    let results = vec![("memory_search".to_string(), "Alpha/Beta".to_string())];

    let panic = catch_unwind(AssertUnwindSafe(|| {
        assert_tool_result_contains(&results, "memory_tree", &["alpha"]);
    }))
    .expect_err("helper should fail when the tool is missing");

    assert_eq!(
        panic_message(panic),
        "Expected at least one result for tool 'memory_tree'"
    );
}

#[test]
fn tool_result_contains_rejects_empty_expected_substrings() {
    let results = vec![("memory_tree".to_string(), "Alpha/Beta".to_string())];

    let panic = catch_unwind(AssertUnwindSafe(|| {
        assert_tool_result_contains(&results, "memory_tree", &[]);
    }))
    .expect_err("helper should reject empty expected substrings");

    assert_eq!(
        panic_message(panic),
        "expected_substrings must not be empty when asserting tool results for 'memory_tree'"
    );
}

#[test]
fn verify_expects_reports_missing_tool_result_with_label() {
    let expects = TraceExpects {
        tool_results_contain: HashMap::from([("memory_tree".to_string(), "alpha".to_string())]),
        ..TraceExpects::default()
    };

    let panic = catch_unwind(AssertUnwindSafe(|| {
        verify_expects(&expects, &[], &[], &[], &[], "turn 0");
    }))
    .expect_err("verify_expects should fail when the tool result is missing");

    assert_eq!(
        panic_message(panic),
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

    let panic = catch_unwind(AssertUnwindSafe(|| {
        verify_expects(&expects, &[], &[], &[], &results, "turn 0");
    }))
    .expect_err("verify_expects should fail when the preview lacks the substring");

    assert_eq!(
        panic_message(panic),
        "[turn 0] tool_results_contain: tool \"memory_tree\" result does not contain \"alpha\", got: \"Gamma/Delta\""
    );
}
