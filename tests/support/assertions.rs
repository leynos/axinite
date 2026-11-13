//! Shared assertion helpers for E2E tests.
//!
//! Extracted from `e2e_spot_checks.rs` so they can be reused across all E2E
//! test files. Mirrors the assertion types from `nearai/benchmarks` SpotSuite.

use regex::Regex;

use crate::support::trace_types::TraceExpects;

/// Assert the response contains all `needles` (case-insensitive).
pub fn assert_response_contains(response: &str, needles: &[&str]) {
    let lower = response.to_lowercase();
    for needle in needles {
        assert!(
            lower.contains(&needle.to_lowercase()),
            "response_contains: missing \"{needle}\" in response: {response}"
        );
    }
}

/// Assert the response matches the given regex `pattern`.
pub fn assert_response_matches(response: &str, pattern: &str) {
    let re = Regex::new(pattern).expect("invalid regex pattern");
    assert!(
        re.is_match(response),
        "response_matches: /{pattern}/ did not match response: {response}"
    );
}

/// Return the first `expected` tool name that does not appear in `started`.
pub fn first_tool_not_used<'a>(started: &[String], expected: &[&'a str]) -> Option<&'a str> {
    expected
        .iter()
        .copied()
        .find(|tool| !started.iter().any(|s| s == tool))
}

/// Return the first `forbidden` tool name that does appear in `started`.
pub fn first_forbidden_tool_used<'a>(started: &[String], forbidden: &[&'a str]) -> Option<&'a str> {
    forbidden
        .iter()
        .copied()
        .find(|tool| started.iter().any(|s| s == tool))
}

/// Assert that all `expected` tool names appear in `started`.
#[track_caller]
pub fn assert_tools_used(started: &[String], expected: &[&str]) {
    if let Some(tool) = first_tool_not_used(started, expected) {
        panic!("tools_used: \"{tool}\" not called, got: {started:?}");
    }
}

/// Assert that none of the `forbidden` tool names appear in `started`.
#[track_caller]
pub fn assert_tools_not_used(started: &[String], forbidden: &[&str]) {
    if let Some(tool) = first_forbidden_tool_used(started, forbidden) {
        panic!("tools_not_used: \"{tool}\" was called, got: {started:?}");
    }
}

/// Assert at most `max` tool calls were started.
pub fn assert_max_tool_calls(started: &[String], max: usize) {
    assert!(
        started.len() <= max,
        "max_tool_calls: expected <= {max}, got {}. Tools: {started:?}",
        started.len()
    );
}

/// Return the names of completed tools that reported failure.
pub fn failed_tools(completed: &[(String, bool)]) -> Vec<&str> {
    completed
        .iter()
        .filter(|(_, success)| !*success)
        .map(|(name, _)| name.as_str())
        .collect()
}

/// Assert ALL completed tools succeeded. Panics listing failed tools.
#[track_caller]
pub fn assert_all_tools_succeeded(completed: &[(String, bool)]) {
    let failed = failed_tools(completed);
    assert!(
        failed.is_empty(),
        "Expected all tools to succeed, but these failed: {failed:?}. All: {completed:?}"
    );
}

/// Assert that at least one preview for `tool_name` contains one of the
/// `expected_substrings`, matching case-insensitively.
///
/// `results` is the captured `(tool_name, preview)` list from the test rig.
/// The helper first filters to entries for `tool_name`, then succeeds when any
/// matching preview contains any expected substring.
///
/// # Panics
///
/// Panics if:
/// - `expected_substrings` is empty,
/// - `expected_substrings` contains a blank or whitespace-only entry,
/// - there are no results for `tool_name`, or
/// - none of the matching results contain any of the expected substrings.
#[track_caller]
pub fn assert_tool_result_contains(
    results: &[(String, String)],
    tool_name: &str,
    expected_substrings: &[&str],
) {
    assert!(
        !expected_substrings.is_empty(),
        "expected_substrings must not be empty when asserting tool results for '{tool_name}'"
    );
    assert!(
        expected_substrings
            .iter()
            .all(|substring| !substring.trim().is_empty()),
        "expected_substrings entries must be non-empty when asserting tool results for '{tool_name}'"
    );

    let matched = matching_tool_results(results, tool_name).unwrap_or_default();
    assert!(
        !matched.is_empty(),
        "Expected at least one result for tool '{tool_name}'"
    );
    assert!(
        any_preview_contains(&matched, expected_substrings),
        "No result for '{tool_name}' contained any of {expected_substrings:?}: {matched:?}"
    );
}

/// Look up the `(tool_name, preview)` entries recorded for `tool_name`.
///
/// Returns `None` when the tool produced no results at all.
pub fn matching_tool_results<'a>(
    results: &'a [(String, String)],
    tool_name: &str,
) -> Option<Vec<&'a (String, String)>> {
    let matched: Vec<_> = results
        .iter()
        .filter(|(name, _)| name == tool_name)
        .collect();
    (!matched.is_empty()).then_some(matched)
}

/// Report whether any preview contains any of `substrings`, ignoring case.
pub fn any_preview_contains(matched: &[&(String, String)], substrings: &[&str]) -> bool {
    let lower_substrings: Vec<String> = substrings
        .iter()
        .map(|substring| substring.to_lowercase())
        .collect();
    matched.iter().any(|(_, preview)| {
        let lower_preview = preview.to_lowercase();
        lower_substrings
            .iter()
            .any(|substring| lower_preview.contains(substring))
    })
}

/// Assert the response does NOT contain any of `forbidden` (case-insensitive).
pub fn assert_response_not_contains(response: &str, forbidden: &[&str]) {
    let lower = response.to_lowercase();
    for needle in forbidden {
        assert!(
            !lower.contains(&needle.to_lowercase()),
            "response_not_contains: found \"{needle}\" in response: {response}"
        );
    }
}

/// Assert that `expected` tools appear in `started` in the given order.
///
/// The tools need not be consecutive — only relative ordering is checked.
/// For example, `assert_tool_order(started, &["write_file", "read_file"])`
/// passes if `write_file` appears before `read_file`, even with other tools
/// in between.
#[track_caller]
pub fn assert_tool_order(started: &[String], expected: &[&str]) {
    if let Some(violation) = tool_order_violation(started, expected) {
        panic!("{violation}");
    }
}

/// Describe the first way `started` violates the `expected` relative order.
///
/// Returns `None` when every expected tool appears in order.
pub fn tool_order_violation(started: &[String], expected: &[&str]) -> Option<String> {
    let mut search_from = 0;
    for tool in expected {
        let found = started[search_from..].iter().position(|s| s == tool);
        match found {
            Some(offset) => search_from += offset + 1,
            None => {
                return Some(format!(
                    "assert_tool_order: \"{tool}\" not found after position {search_from} \
                     in: {started:?}. Expected order: {expected:?}"
                ));
            }
        }
    }
    None
}

/// Verify all expectations from a `TraceExpects` against actual data.
///
/// `label` is used in assertion messages to identify context (e.g. "top-level" or "turn 0").
/// `responses` are the response content strings, `started` are tool names started,
/// `completed` are (name, success) pairs, `results` are (name, preview) pairs.
pub fn verify_expects(
    expects: &TraceExpects,
    responses: &[String],
    started: &[String],
    completed: &[(String, bool)],
    results: &[(String, String)],
    label: &str,
) {
    if expects.is_empty() {
        return;
    }

    // min_responses
    if let Some(min) = expects.min_responses {
        assert!(
            responses.len() >= min,
            "[{label}] min_responses: expected >= {min}, got {}",
            responses.len()
        );
    }

    // response_contains / response_not_contains / response_matches — checked against joined response
    let joined = responses.join("\n");

    if !expects.response_contains.is_empty() {
        let needles: Vec<&str> = expects
            .response_contains
            .iter()
            .map(|s| s.as_str())
            .collect();
        assert_response_contains(&joined, &needles);
    }

    if !expects.response_not_contains.is_empty() {
        let forbidden: Vec<&str> = expects
            .response_not_contains
            .iter()
            .map(|s| s.as_str())
            .collect();
        assert_response_not_contains(&joined, &forbidden);
    }

    if let Some(ref pattern) = expects.response_matches {
        assert_response_matches(&joined, pattern);
    }

    // tools_used
    if !expects.tools_used.is_empty() {
        let expected: Vec<&str> = expects.tools_used.iter().map(|s| s.as_str()).collect();
        assert_tools_used(started, &expected);
    }

    // tools_not_used
    if !expects.tools_not_used.is_empty() {
        let forbidden: Vec<&str> = expects.tools_not_used.iter().map(|s| s.as_str()).collect();
        assert_tools_not_used(started, &forbidden);
    }

    // all_tools_succeeded
    if expects.all_tools_succeeded == Some(true) {
        assert_all_tools_succeeded(completed);
    }

    // max_tool_calls
    if let Some(max) = expects.max_tool_calls {
        assert_max_tool_calls(started, max);
    }

    // tools_order
    if !expects.tools_order.is_empty() {
        let expected: Vec<&str> = expects.tools_order.iter().map(|s| s.as_str()).collect();
        assert_tool_order(started, &expected);
    }

    // tool_results_contain
    for (tool_name, substring) in &expects.tool_results_contain {
        let found = results.iter().find(|(name, _)| name == tool_name);
        let Some((_, preview)) = found else {
            panic!(
                "[{label}] tool_results_contain: no result for tool \"{tool_name}\", got: {results:?}"
            );
        };
        assert!(
            preview.to_lowercase().contains(&substring.to_lowercase()),
            "[{label}] tool_results_contain: tool \"{tool_name}\" result does not contain \"{substring}\", got: \"{preview}\""
        );
    }
}
