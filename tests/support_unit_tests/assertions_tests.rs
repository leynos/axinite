//! Unit tests for the shared assertion helpers in
//! [`crate::support::assertions`].
//!
//! These tests cover case-insensitive substring matching, per-tool result
//! filtering, and panic-message diagnostics checked with
//! `#[should_panic(expected = "…")]`. They also exercise edge-case guards
//! such as empty `expected_substrings` inputs and the `TraceExpects`-driven
//! checks in `verify_expects`.

use std::collections::HashMap;

use rstest::rstest;

use crate::support::assertions::*;
use crate::support::trace_types::TraceExpects;

#[rstest]
#[case("The agent found Alpha and Beta.", &["alpha", "BETA"])]
fn response_contains_passes(#[case] input: &str, #[case] patterns: &'static [&'static str]) {
    assert_response_contains(input, patterns);
}

#[test]
#[should_panic(expected = "response_contains")]
fn response_contains_panics_when_pattern_is_missing() {
    assert_response_contains("The agent found Alpha.", &["gamma"]);
}

#[test]
fn response_matches_passes_for_matching_pattern() {
    assert_response_matches("job-123 completed", r"job-\d+ completed");
}

#[test]
#[should_panic(expected = "response_matches")]
fn response_matches_panics_for_non_matching_pattern() {
    assert_response_matches("job pending", r"job-\d+ completed");
}

#[test]
fn response_not_contains_passes_when_patterns_are_absent() {
    assert_response_not_contains("The request succeeded.", &["failed", "ERROR"]);
}

#[test]
#[should_panic(expected = "response_not_contains")]
fn response_not_contains_panics_when_pattern_is_present() {
    assert_response_not_contains("The request failed.", &["FAILED"]);
}

#[test]
fn tools_used_passes_when_all_expected_tools_ran() {
    let started = vec!["read_file".to_string(), "write_file".to_string()];
    assert_tools_used(&started, &["read_file", "write_file"]);
}

#[test]
#[should_panic(expected = "tools_used")]
fn tools_used_panics_when_an_expected_tool_did_not_run() {
    let started = vec!["read_file".to_string()];
    assert_tools_used(&started, &["write_file"]);
}

#[test]
fn tools_not_used_passes_when_no_forbidden_tool_ran() {
    let started = vec!["read_file".to_string()];
    assert_tools_not_used(&started, &["delete_file"]);
}

#[test]
#[should_panic(expected = "tools_not_used")]
fn tools_not_used_panics_when_a_forbidden_tool_ran() {
    let started = vec!["delete_file".to_string()];
    assert_tools_not_used(&started, &["delete_file"]);
}

#[test]
fn max_tool_calls_passes_at_the_limit() {
    let started = vec!["read_file".to_string(), "write_file".to_string()];
    assert_max_tool_calls(&started, 2);
}

#[test]
#[should_panic(expected = "max_tool_calls")]
fn max_tool_calls_panics_above_the_limit() {
    let started = vec!["read_file".to_string(), "write_file".to_string()];
    assert_max_tool_calls(&started, 1);
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
#[should_panic(
    expected = r#"No result for 'memory_tree' contained any of ["alpha"]: [("memory_tree", "Gamma/Delta")]"#
)]
fn tool_result_contains_ignores_other_tools_when_matching() {
    let results = vec![
        ("memory_search".to_string(), "Alpha/Beta".to_string()),
        ("memory_tree".to_string(), "Gamma/Delta".to_string()),
    ];

    assert_tool_result_contains(&results, "memory_tree", &["alpha"]);
}

#[test]
#[should_panic(expected = "Expected at least one result for tool 'memory_tree'")]
fn tool_result_contains_panics_when_tool_is_missing() {
    let results = vec![("memory_search".to_string(), "Alpha/Beta".to_string())];

    assert_tool_result_contains(&results, "memory_tree", &["alpha"]);
}

#[test]
#[should_panic(
    expected = "expected_substrings must not be empty when asserting tool results for 'memory_tree'"
)]
fn tool_result_contains_rejects_empty_expected_substrings() {
    let results = vec![("memory_tree".to_string(), "Alpha/Beta".to_string())];

    assert_tool_result_contains(&results, "memory_tree", &[]);
}

#[test]
#[should_panic(
    expected = "expected_substrings entries must be non-empty when asserting tool results for 'memory_tree'"
)]
fn tool_result_contains_rejects_blank_expected_substrings() {
    let results = vec![("memory_tree".to_string(), "Alpha/Beta".to_string())];

    assert_tool_result_contains(&results, "memory_tree", &["   "]);
}

fn memory_tree_expects() -> TraceExpects {
    TraceExpects {
        tool_results_contain: HashMap::from([("memory_tree".to_string(), "alpha".to_string())]),
        ..TraceExpects::default()
    }
}

#[test]
#[should_panic(
    expected = r#"[turn 0] tool_results_contain: no result for tool "memory_tree", got: []"#
)]
fn verify_expects_reports_missing_tool_result_with_label() {
    verify_expects(&memory_tree_expects(), &[], &[], &[], &[], "turn 0");
}

#[test]
#[should_panic(
    expected = r#"[turn 0] tool_results_contain: tool "memory_tree" result does not contain "alpha", got: "Gamma/Delta""#
)]
fn verify_expects_reports_missing_substring_with_preview() {
    let results = vec![("memory_tree".to_string(), "Gamma/Delta".to_string())];

    verify_expects(&memory_tree_expects(), &[], &[], &[], &results, "turn 0");
}
