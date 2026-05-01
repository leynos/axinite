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

use insta::assert_snapshot;
use rstest::rstest;

use crate::support::assertions::*;
use crate::support::trace_types::TraceExpects;

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
fn panic_message_reports_non_string_payloads() {
    let payload = catch_unwind(|| std::panic::panic_any(42_u32))
        .expect_err("panic_any should produce a panic payload");

    assert_eq!(panic_message(payload), "non-string panic payload");
}

fn assert_panics_with_message<F>(f: F, expected_message: &str)
where
    F: FnOnce() + std::panic::UnwindSafe,
{
    let payload = catch_unwind(f).expect_err("expected a panic but none occurred");
    assert_eq!(panic_message(payload), expected_message);
}

fn capture_panic_message<F>(f: F) -> String
where
    F: FnOnce() + UnwindSafe,
{
    let panic = catch_unwind(AssertUnwindSafe(f)).expect_err("helper should panic");
    panic_message(panic)
}

#[rstest]
#[case(
    "The agent found Alpha and Beta.",
    &["alpha", "BETA"],
    None
)]
#[case(
    "The agent found Alpha.",
    &["gamma"],
    Some("response_contains")
)]
fn response_contains_cases(
    #[case] input: &str,
    #[case] patterns: &'static [&'static str],
    #[case] expected_panic: Option<&str>,
) {
    match expected_panic {
        Some(expected_panic) => {
            let message = capture_panic_message(|| assert_response_contains(input, patterns));
            assert!(
                message.contains(expected_panic),
                "expected panic containing {expected_panic:?}, got {message:?}"
            );
        }
        None => assert_response_contains(input, patterns),
    }
}

#[rstest]
#[case("job-123 completed", r"job-\d+ completed", false)]
#[case("job pending", r"job-\d+ completed", true)]
fn response_matches_cases(#[case] input: &str, #[case] pattern: &str, #[case] expect_panic: bool) {
    if expect_panic {
        let message = capture_panic_message(|| assert_response_matches(input, pattern));
        assert!(
            message.contains("response_matches"),
            "expected response_matches panic, got {message:?}"
        );
    } else {
        assert_response_matches(input, pattern);
    }
}

#[rstest]
#[case(
    "The request succeeded.",
    &["failed", "ERROR"],
    None
)]
#[case(
    "The request failed.",
    &["FAILED"],
    Some("response_not_contains")
)]
fn response_not_contains_cases(
    #[case] input: &str,
    #[case] patterns: &'static [&'static str],
    #[case] expected_panic: Option<&str>,
) {
    match expected_panic {
        Some(expected_panic) => {
            let message = capture_panic_message(|| assert_response_not_contains(input, patterns));
            assert!(
                message.contains(expected_panic),
                "expected panic containing {expected_panic:?}, got {message:?}"
            );
        }
        None => assert_response_not_contains(input, patterns),
    }
}

#[rstest]
#[case(
    vec!["read_file".to_string(), "write_file".to_string()],
    &["read_file", "write_file"],
    None
)]
#[case(
    vec!["read_file".to_string()],
    &["write_file"],
    Some("tools_used")
)]
fn tools_used_cases(
    #[case] started: Vec<String>,
    #[case] expected: &'static [&'static str],
    #[case] expected_panic: Option<&str>,
) {
    match expected_panic {
        Some(expected_panic) => {
            let message = capture_panic_message(|| assert_tools_used(&started, expected));
            assert!(
                message.contains(expected_panic),
                "expected panic containing {expected_panic:?}, got {message:?}"
            );
        }
        None => assert_tools_used(&started, expected),
    }
}

#[rstest]
#[case(vec!["read_file".to_string()], &["delete_file"], None)]
#[case(vec!["delete_file".to_string()], &["delete_file"], Some("tools_not_used"))]
fn tools_not_used_cases(
    #[case] started: Vec<String>,
    #[case] forbidden: &'static [&'static str],
    #[case] expected_panic: Option<&str>,
) {
    match expected_panic {
        Some(expected_panic) => {
            let message = capture_panic_message(|| assert_tools_not_used(&started, forbidden));
            assert!(
                message.contains(expected_panic),
                "expected panic containing {expected_panic:?}, got {message:?}"
            );
        }
        None => assert_tools_not_used(&started, forbidden),
    }
}

#[rstest]
#[case(vec!["read_file".to_string(), "write_file".to_string()], 2, None)]
#[case(
    vec!["read_file".to_string(), "write_file".to_string()],
    1,
    Some("max_tool_calls")
)]
fn max_tool_calls_cases(
    #[case] started: Vec<String>,
    #[case] limit: usize,
    #[case] expected_panic: Option<&str>,
) {
    match expected_panic {
        Some(expected_panic) => {
            let message = capture_panic_message(|| assert_max_tool_calls(&started, limit));
            assert!(
                message.contains(expected_panic),
                "expected panic containing {expected_panic:?}, got {message:?}"
            );
        }
        None => assert_max_tool_calls(&started, limit),
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

    assert_snapshot!(panic);
}

#[test]
fn tool_result_contains_panics_when_tool_is_missing() {
    let results = vec![("memory_search".to_string(), "Alpha/Beta".to_string())];

    assert_panics_with_message(
        AssertUnwindSafe(|| {
            assert_tool_result_contains(&results, "memory_tree", &["alpha"]);
        }),
        "Expected at least one result for tool 'memory_tree'",
    );
}

#[rstest]
#[case(
    &[],
    "expected_substrings must not be empty when asserting tool results for 'memory_tree'"
)]
#[case(
    &["   "],
    "expected_substrings entries must be non-empty when asserting tool results for 'memory_tree'"
)]
fn tool_result_contains_rejects_invalid_expected_substrings(
    #[case] expected_substrings: &'static [&'static str],
    #[case] expected_message: &str,
) {
    let results = vec![("memory_tree".to_string(), "Alpha/Beta".to_string())];

    assert_panics_with_message(
        AssertUnwindSafe(|| {
            assert_tool_result_contains(&results, "memory_tree", expected_substrings);
        }),
        expected_message,
    );
}

#[rstest]
#[case("verify_expects_reports_missing_tool_result_with_label", vec![])]
#[case(
    "verify_expects_reports_missing_substring_with_preview",
    vec![("memory_tree".to_string(), "Gamma/Delta".to_string())]
)]
fn verify_expects_reports_tool_result_failure(
    #[case] snapshot_name: &str,
    #[case] results: Vec<(String, String)>,
) {
    let expects = TraceExpects {
        tool_results_contain: HashMap::from([("memory_tree".to_string(), "alpha".to_string())]),
        ..TraceExpects::default()
    };

    let message = capture_panic_message(AssertUnwindSafe(|| {
        verify_expects(&expects, &[], &[], &[], &results, "turn 0");
    }));

    assert_snapshot!(snapshot_name, message);
}
