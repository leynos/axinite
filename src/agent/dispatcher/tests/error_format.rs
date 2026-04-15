//! Error formatting and text stripping tests.

use insta::assert_snapshot;
use rstest::rstest;

use super::super::types::strip_internal_tool_call_text;

#[rstest]
#[case::called_marker(
    "[Called tool search({\"query\": \"test\"})]\nHere is the answer.",
    "Here is the answer."
)]
#[case::returned_marker(
    "[Tool search returned: some result]\nSummary of findings.",
    "Summary of findings."
)]
#[case::normal_text(
    "This is a normal response with [brackets] inside.",
    "This is a normal response with [brackets] inside."
)]
fn test_strip_internal_tool_call_text_cases(#[case] input: &str, #[case] expected: &str) {
    let result = strip_internal_tool_call_text(input);
    assert_eq!(result, expected);
}

#[test]
fn test_strip_internal_tool_call_text_all_markers_yields_fallback_snapshot() {
    let input = "[Called tool search({\"query\": \"test\"})]\n[Tool search returned: error]";
    let result = strip_internal_tool_call_text(input);
    assert_snapshot!("strip_all_markers_fallback", result);
}

#[test]
fn strip_internal_markers_snapshot_markers_only() {
    let input = "[Called tool `x`]\n[Tool `x` returned: ...]";
    let out = crate::agent::dispatcher::types::strip_internal_tool_call_text(input);
    assert_snapshot!("strip_markers_only", out);
}

#[test]
fn test_tool_error_format_includes_tool_name() {
    // Regression test for issue #487: tool errors sent to the LLM should
    // include the tool name so the model can reason about which tool failed
    // and try alternatives.
    let tool_name = "http";
    let err = crate::error::ToolError::ExecutionFailed {
        name: tool_name.to_string(),
        reason: "connection refused".to_string(),
    };
    let formatted = err.to_string();
    assert!(
        formatted.contains(tool_name),
        "Error should identify the tool by name, got: {formatted}"
    );
    assert!(
        formatted.contains("connection refused"),
        "Error should include the underlying reason, got: {formatted}"
    );
}
