//! Error formatting and text stripping tests.

use super::super::types::strip_internal_tool_call_text;

#[test]
fn test_strip_internal_tool_call_text_removes_markers() {
    let input = "[Called tool search({\"query\": \"test\"})]\nHere is the answer.";
    let result = strip_internal_tool_call_text(input);
    assert_eq!(result, "Here is the answer.");
}

#[test]
fn test_strip_internal_tool_call_text_removes_returned_markers() {
    let input = "[Tool search returned: some result]\nSummary of findings.";
    let result = strip_internal_tool_call_text(input);
    assert_eq!(result, "Summary of findings.");
}

#[test]
fn test_strip_internal_tool_call_text_all_markers_yields_fallback() {
    let input = "[Called tool search({\"query\": \"test\"})]\n[Tool search returned: error]";
    let result = strip_internal_tool_call_text(input);
    assert!(result.contains("wasn't able to complete"));
}

#[test]
fn test_strip_internal_tool_call_text_preserves_normal_text() {
    let input = "This is a normal response with [brackets] inside.";
    let result = strip_internal_tool_call_text(input);
    assert_eq!(result, input);
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
    let formatted = format!("Tool '{}' failed: {}", tool_name, err);
    assert!(
        formatted.contains("Tool 'http' failed:"),
        "Error should identify the tool by name, got: {formatted}"
    );
    assert!(
        formatted.contains("connection refused"),
        "Error should include the underlying reason, got: {formatted}"
    );
}
