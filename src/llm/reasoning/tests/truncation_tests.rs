//! Tests for truncation at unclosed tool-call tags (issue #789), including
//! code-awareness, case-insensitivity, and `closing_tag_for` derivation.

use super::super::truncation::{TOOL_TAG_PATTERNS, closing_tag_for};
use super::*;

// ---- Issue #789: truncate_at_tool_tags tests ----

#[test]
fn test_truncate_preserves_text_before_tool_tag() {
    let input = "Here is my answer about the topic.\n<tool_call>{\"name\": \"search\"}";
    assert_eq!(
        truncate_at_tool_tags(input),
        "Here is my answer about the topic.\n"
    );
}

#[test]
fn test_truncate_no_tool_tags_unchanged() {
    let input = "Just a normal response with no tool tags.";
    assert_eq!(truncate_at_tool_tags(input), input);
}

#[test]
fn test_truncate_empty_string() {
    assert_eq!(truncate_at_tool_tags(""), "");
}

#[test]
fn test_truncate_tool_tag_at_start() {
    assert_eq!(
        truncate_at_tool_tags("<tool_call>{\"name\": \"search\"}"),
        ""
    );
}

#[test]
fn test_truncate_picks_earliest_unclosed_tag() {
    // <function_call>...</function_call> is closed — skipped.
    // <tool_call>second is unclosed — truncated here.
    let input = "Text before <function_call>first</function_call> and <tool_call>second";
    assert_eq!(
        truncate_at_tool_tags(input),
        "Text before <function_call>first</function_call> and "
    );
}

#[test]
fn test_truncate_pipe_delimited_tags() {
    let input = "Answer here\n<|tool_call|>{\"name\": \"fetch\"}";
    assert_eq!(truncate_at_tool_tags(input), "Answer here\n");
}

#[test]
fn test_truncate_closed_tag_with_attributes_preserved() {
    // Closed tag (even with attributes) is left for clean_response()
    let input = "Some text <tool_call id=\"123\">{\"name\": \"test\"}</tool_call>";
    assert_eq!(truncate_at_tool_tags(input), input);
}

#[test]
fn test_truncate_unclosed_tag_with_attributes() {
    let input = "Some text <tool_call id=\"123\">{\"name\": \"test\"}";
    assert_eq!(truncate_at_tool_tags(input), "Some text ");
}

#[test]
fn test_truncate_whitespace_only_before_tag() {
    assert_eq!(truncate_at_tool_tags("   \n\n<tool_call>{}"), "   \n\n");
}

#[test]
fn test_truncate_ignores_tags_inside_code_blocks() {
    let input = "Here's the XML format:\n\n```xml\n<tool_call>{\"name\": \"search\"}</tool_call>\n```\n\nYou can use this to call tools.";
    assert_eq!(truncate_at_tool_tags(input), input);
}

#[test]
fn test_truncate_finds_tag_after_code_block() {
    let input = "Example:\n\n```\n<tool_call>example</tool_call>\n```\n\nReal output:\n<tool_call>{\"name\": \"x\"}";
    assert_eq!(
        truncate_at_tool_tags(input),
        "Example:\n\n```\n<tool_call>example</tool_call>\n```\n\nReal output:\n"
    );
}

// ---- Issue #789: full pipeline (truncate + clean_response) tests ----

#[test]
fn test_issue_789_force_text_unclosed_tool_tag() {
    let model_output = "The file contains a main function that initializes the server.\n<tool_call>{\"name\": \"read_file\", \"arguments\": {\"path\": \"src/main.rs\"}}";
    let pre_truncated = truncate_at_tool_tags(model_output);
    let cleaned = clean_response(&pre_truncated);
    assert_eq!(
        cleaned,
        "The file contains a main function that initializes the server."
    );
}

#[test]
fn test_issue_789_only_tool_tag_produces_empty() {
    let model_output = "<tool_call>{\"name\": \"search\", \"arguments\": {\"q\": \"test\"}}";
    let pre_truncated = truncate_at_tool_tags(model_output);
    let cleaned = clean_response(&pre_truncated);
    assert!(cleaned.trim().is_empty());
}

#[test]
fn test_issue_789_thinking_then_tool_tag() {
    let model_output =
        "<think>I should search for this</think>Let me help you.\n<tool_call>{\"name\": \"s\"}";
    let pre_truncated = truncate_at_tool_tags(model_output);
    let cleaned = clean_response(&pre_truncated);
    assert_eq!(cleaned, "Let me help you.");
}

#[test]
fn test_issue_789_closed_tool_tag_preserved_for_clean_response() {
    // Closed tags are left intact — clean_response() strips them normally,
    // preserving any text after the tag.
    let model_output = "Info here.\n<tool_call>{\"name\": \"x\"}</tool_call>\nMore text.";
    let pre_truncated = truncate_at_tool_tags(model_output);
    assert_eq!(
        pre_truncated, model_output,
        "Closed tag should not be truncated"
    );
    let cleaned = clean_response(&pre_truncated);
    assert_eq!(cleaned, "Info here.\n\nMore text.");
}

// ---- Issue #789: additional edge case tests for truncate_at_tool_tags ----

#[test]
fn test_truncate_unicode_content_before_tool_tag() {
    let input = "こんにちは世界！素晴らしい結果です。\n<tool_call>{\"name\": \"search\"}";
    assert_eq!(
        truncate_at_tool_tags(input),
        "こんにちは世界！素晴らしい結果です。\n"
    );
}

#[test]
fn test_truncate_emoji_content_preserved() {
    let input = "The answer is 42 🎉🚀\n<function_call>{\"name\": \"x\"}";
    assert_eq!(truncate_at_tool_tags(input), "The answer is 42 🎉🚀\n");
}

#[test]
fn test_truncate_very_long_text_before_tag() {
    let long_text = "A".repeat(10_000);
    let input = format!("{}\n<tool_call>{{\"name\": \"x\"}}", long_text);
    let result = truncate_at_tool_tags(&input);
    assert_eq!(result.len(), long_text.len() + 1); // +1 for \n
    assert!(result.starts_with("AAAA"));
}

#[test]
fn test_truncate_multiple_code_blocks_with_tags() {
    let input = "Explanation:\n\n```python\n# <tool_call> in comment\nprint('hi')\n```\n\nAnd also:\n\n```xml\n<function_call>example</function_call>\n```\n\nFinal answer here.";
    // Both tags are inside code blocks, so nothing is truncated
    assert_eq!(truncate_at_tool_tags(input), input);
}

#[test]
fn test_truncate_inline_code_with_tool_tag() {
    let input = "Use `<tool_call>` to invoke tools.\n<tool_call>{\"name\": \"real\"}";
    // First occurrence is in inline code, second is real
    assert_eq!(
        truncate_at_tool_tags(input),
        "Use `<tool_call>` to invoke tools.\n"
    );
}

#[test]
fn test_truncate_tag_immediately_after_code_block() {
    let input = "```\nexample\n```\n<tool_call>{\"name\": \"x\"}";
    assert_eq!(truncate_at_tool_tags(input), "```\nexample\n```\n");
}

#[test]
fn test_truncate_interleaved_thinking_and_tool_tags() {
    // Simulate: thinking tag + text + tool tag
    let input = "<think>reasoning</think>Here's the answer.\n<tool_call>{\"name\": \"y\"}";
    let truncated = truncate_at_tool_tags(input);
    let cleaned = clean_response(&truncated);
    assert_eq!(cleaned, "Here's the answer.");
}

#[test]
fn test_truncate_closed_tool_calls_plural_preserved() {
    // Closed <tool_calls>...</tool_calls> left for clean_response()
    let input = "Answer.\n<tool_calls>[{\"name\": \"a\"}, {\"name\": \"b\"}]</tool_calls>";
    assert_eq!(truncate_at_tool_tags(input), input);
}

#[test]
fn test_truncate_unclosed_tool_calls_plural() {
    let input = "Answer.\n<tool_calls>[{\"name\": \"a\"}, {\"name\": \"b\"}]";
    assert_eq!(truncate_at_tool_tags(input), "Answer.\n");
}

#[test]
fn test_truncate_closed_pipe_function_call_preserved() {
    let input = "Done!\n<|function_call|>{\"name\": \"x\"}<|/function_call|>";
    assert_eq!(truncate_at_tool_tags(input), input);
}

#[test]
fn test_truncate_unclosed_pipe_function_call() {
    let input = "Done!\n<|function_call|>{\"name\": \"x\"}";
    assert_eq!(truncate_at_tool_tags(input), "Done!\n");
}

#[test]
fn test_truncate_adversarial_nested_code_blocks() {
    // Adversarial: code block inside another structure
    let input = "```\nouter\n```\n\nReal text.\n\n```\n<tool_call>inside</tool_call>\n```\n\n<tool_call>{\"name\": \"real\"}";
    let result = truncate_at_tool_tags(input);
    assert!(result.contains("Real text."));
    assert!(!result.contains("{\"name\": \"real\"}"));
}

// ---- Issue #789: case-insensitive truncation ----

#[test]
fn test_truncate_case_insensitive_upper() {
    let input = "Some answer.\n<TOOL_CALL>{\"name\": \"search\"}";
    assert_eq!(truncate_at_tool_tags(input), "Some answer.\n");
}

#[test]
fn test_truncate_case_insensitive_mixed() {
    let input = "Result here.\n<Tool_Call>{\"name\": \"x\"}";
    assert_eq!(truncate_at_tool_tags(input), "Result here.\n");
}

#[test]
fn test_truncate_unicode_before_case_insensitive_tag_no_panic() {
    // Regression: to_lowercase() can change byte lengths for non-ASCII chars
    // (e.g. Kelvin sign U+212A is 3 bytes, lowercases to 'k' which is 1 byte).
    // Using to_ascii_lowercase() keeps byte offsets stable.
    let input = "Ответ: 42\n<TOOL_CALL>{\"name\": \"x\"}";
    assert_eq!(truncate_at_tool_tags(input), "Ответ: 42\n");
}

#[test]
fn test_truncate_case_insensitive_function_call_closed() {
    // Closed tag (case-insensitive) preserved for clean_response()
    let input = "Done.\n<FUNCTION_CALL>{\"name\": \"y\"}</FUNCTION_CALL>";
    assert_eq!(truncate_at_tool_tags(input), input);
}

#[test]
fn test_truncate_case_insensitive_function_call_unclosed() {
    let input = "Done.\n<FUNCTION_CALL>{\"name\": \"y\"}";
    assert_eq!(truncate_at_tool_tags(input), "Done.\n");
}

// ---- closing_tag_for() unit tests ----

#[test]
fn test_closing_tag_for_standard_tags() {
    assert_eq!(
        closing_tag_for("<tool_call>").as_deref(),
        Some("</tool_call>")
    );
    assert_eq!(
        closing_tag_for("<function_call>").as_deref(),
        Some("</function_call>")
    );
    assert_eq!(
        closing_tag_for("<tool_calls>").as_deref(),
        Some("</tool_calls>")
    );
}

#[test]
fn test_closing_tag_for_space_suffixed_patterns() {
    // Patterns with trailing space (for attribute matching)
    assert_eq!(
        closing_tag_for("<tool_call ").as_deref(),
        Some("</tool_call>")
    );
    assert_eq!(
        closing_tag_for("<function_call ").as_deref(),
        Some("</function_call>")
    );
    assert_eq!(
        closing_tag_for("<tool_calls ").as_deref(),
        Some("</tool_calls>")
    );
}

#[test]
fn test_closing_tag_for_pipe_delimited() {
    assert_eq!(
        closing_tag_for("<|tool_call|>").as_deref(),
        Some("<|/tool_call|>")
    );
    assert_eq!(
        closing_tag_for("<|function_call|>").as_deref(),
        Some("<|/function_call|>")
    );
    assert_eq!(
        closing_tag_for("<|tool_calls|>").as_deref(),
        Some("<|/tool_calls|>")
    );
}

#[test]
fn test_closing_tag_for_covers_all_patterns() {
    // Every entry in TOOL_TAG_PATTERNS must produce a closing tag
    for pattern in TOOL_TAG_PATTERNS {
        assert!(
            closing_tag_for(pattern).is_some(),
            "closing_tag_for({:?}) returned None",
            pattern
        );
    }
}

// ---- truncation with multiple tags: first closed, second unclosed ----

#[test]
fn test_truncate_mixed_closed_then_unclosed_different_types() {
    let input = "Text <function_call>{}</function_call> middle <tool_call>{\"name\": \"x\"}";
    // function_call is closed → skipped. tool_call is unclosed → truncated.
    assert_eq!(
        truncate_at_tool_tags(input),
        "Text <function_call>{}</function_call> middle "
    );
}
