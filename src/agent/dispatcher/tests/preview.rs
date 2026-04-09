//! Preview/truncation tests.

use super::super::types::truncate_for_preview;

#[test]
fn test_truncate_short_input() {
    assert_eq!(truncate_for_preview("hello", 10), "hello");
}

#[test]
fn test_truncate_empty_input() {
    assert_eq!(truncate_for_preview("", 10), "");
}

#[test]
fn test_truncate_exact_length() {
    assert_eq!(truncate_for_preview("hello", 5), "hello");
}

#[test]
fn test_truncate_over_limit() {
    let result = truncate_for_preview("hello world, this is long", 10);
    assert!(result.ends_with("..."));
    assert_eq!(result, "hello worl...");
}

#[test]
fn test_truncate_collapses_newlines() {
    let result = truncate_for_preview("line1\nline2\nline3", 100);
    assert!(!result.contains('\n'));
    assert_eq!(result, "line1 line2 line3");
}

#[test]
fn test_truncate_collapses_whitespace() {
    let result = truncate_for_preview("hello   world", 100);
    assert_eq!(result, "hello world");
}

#[test]
fn test_truncate_multibyte_utf8() {
    let input = "😀😁😂🤣😃😄😅😆😉😊";
    let result = truncate_for_preview(input, 5);
    assert!(result.ends_with("..."));
    assert_eq!(result, "😀😁😂🤣😃...");
}

#[test]
fn test_truncate_cjk_characters() {
    let input = "你好世界测试数据很长的字符串";
    let result = truncate_for_preview(input, 4);
    assert_eq!(result, "你好世界...");
}

#[test]
fn test_truncate_mixed_multibyte_and_ascii() {
    let input = "hello 世界 foo";
    let result = truncate_for_preview(input, 8);
    assert_eq!(result, "hello 世界...");
}

#[test]
fn test_truncate_large_whitespace_run_does_not_hide_content() {
    // "A" followed by 101 newlines then "B": after normalisation this is "A B" (3 chars).
    let input = format!("A{}\nB", "\n".repeat(100));
    assert_eq!(truncate_for_preview(&input, 3), "A B");
}

#[test]
fn test_truncate_large_whitespace_run_truncates_correctly() {
    // 100 newlines between words: normalise to "A B C", cap at 3 → "A B..."
    let input = format!("A{}B{}C", "\n".repeat(100), "\n".repeat(100));
    let result = truncate_for_preview(&input, 3);
    assert_eq!(result, "A B...");
}
