//! Tests for code-aware cleaning: tags preserved inside code regions,
//! `<final>` extraction, unicode handling, newline collapsing, and code
//! region detection.

use super::*;

// ---- Code-awareness: tags inside code blocks are preserved ----

#[test]
fn test_tags_in_fenced_code_block_preserved() {
    let input =
        "Here is an example:\n\n```\n<thinking>This is inside code</thinking>\n```\n\nDone.";
    assert_eq!(clean_response(input), input);
}

#[test]
fn test_tags_in_tilde_fenced_block_preserved() {
    let input = "Example:\n\n~~~\n<think>code example</think>\n~~~\n\nEnd.";
    assert_eq!(clean_response(input), input);
}

#[test]
fn test_tags_in_inline_backticks_preserved() {
    let input = "Use the `<thinking>` tag for reasoning.";
    assert_eq!(clean_response(input), input);
}

#[test]
fn test_mixed_real_and_code_tags() {
    let input = "<thinking>real reasoning</thinking>Use `<thinking>` tags.\n\n```\n<thinking>code example</thinking>\n```";
    let expected = "Use `<thinking>` tags.\n\n```\n<thinking>code example</thinking>\n```";
    assert_eq!(clean_response(input), expected);
}

#[test]
fn test_code_block_with_info_string() {
    let input = "```xml\n<thinking>xml example</thinking>\n```\nVisible.";
    assert_eq!(clean_response(input), input);
}

// ---- <final> tag extraction ----

#[test]
fn test_final_tag_basic() {
    let input = "<think>reasoning</think><final>answer</final>";
    assert_eq!(clean_response(input), "answer");
}

#[test]
fn test_final_tag_strips_untagged_reasoning() {
    let input = "Untagged reasoning.\n<final>answer</final>";
    assert_eq!(clean_response(input), "answer");
}

#[test]
fn test_final_tag_multiple_blocks() {
    let input =
        "<think>part 1</think><final>Hello </final><think>part 2</think><final>world!</final>";
    assert_eq!(clean_response(input), "Hello world!");
}

#[test]
fn test_no_final_tag_fallthrough() {
    // Without <final>, thinking-stripped text returned as-is
    let input = "<think>reasoning</think>Just the answer.";
    assert_eq!(clean_response(input), "Just the answer.");
}

#[test]
fn test_no_tags_at_all() {
    let input = "Just a normal response";
    assert_eq!(clean_response(input), input);
}

#[test]
fn test_final_tag_in_code_preserved() {
    // <final> inside code block should not trigger extraction
    let input = "Use `<final>` to mark output.\n\nHello.";
    assert_eq!(clean_response(input), input);
}

#[test]
fn test_final_tag_unclosed_includes_trailing() {
    let input = "<think>reasoning</think><final>answer continues";
    assert_eq!(clean_response(input), "answer continues");
}

// ---- Unicode content ----

#[test]
fn test_unicode_content_preserved() {
    let input = "<thinking>日本語の推論</thinking>こんにちは世界！";
    assert_eq!(clean_response(input), "こんにちは世界！");
}

#[test]
fn test_unicode_in_final() {
    let input = "<think>推論</think><final>答え：42</final>";
    assert_eq!(clean_response(input), "答え：42");
}

// ---- Newline collapsing ----

#[test]
fn test_collapse_triple_newlines() {
    let input = "<thinking>removed</thinking>\n\n\nVisible.";
    assert_eq!(clean_response(input), "Visible.");
}

#[test]
fn test_trims_whitespace() {
    let input = "  <thinking>removed</thinking>  Hello, user!  \n";
    assert_eq!(clean_response(input), "Hello, user!");
}

// ---- Code region detection ----

#[test]
fn test_find_code_regions_fenced() {
    let text = "before\n```\ncode\n```\nafter";
    let regions = find_code_regions(text);
    assert_eq!(regions.len(), 1);
    assert!(text[regions[0].start..regions[0].end].contains("code"));
}

#[test]
fn test_find_code_regions_inline() {
    let text = "Use `<thinking>` tag.";
    let regions = find_code_regions(text);
    assert_eq!(regions.len(), 1);
    assert!(text[regions[0].start..regions[0].end].contains("<thinking>"));
}

#[test]
fn test_find_code_regions_unclosed_fence() {
    let text = "before\n```\ncode goes on\nno closing fence";
    let regions = find_code_regions(text);
    assert_eq!(regions.len(), 1);
    // Unclosed fence extends to EOF
    assert_eq!(regions[0].end, text.len());
}
