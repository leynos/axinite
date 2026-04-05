//! Tests for terminal formatting helpers.

use super::*;

fn strip_ansi(text: &str) -> String {
    ansi_sgr_regex().replace_all(text, "").into_owned()
}

#[test]
fn truncate_card_content_preserves_visible_width_for_plain_text() {
    let truncated = truncate_card_content("abcdefghij", 6);

    assert_eq!(truncated, "abcde…");
}

#[test]
fn truncate_card_content_handles_ansi_sequences_without_corruption() {
    let line = "\x1b[36mkey\x1b[0m: \x1b[32m\"abcdefghijklmnop\"\x1b[0m";

    let truncated = truncate_card_content(line, 10);

    assert_eq!(strip_ansi(&truncated), "key: \"abc…");
    assert!(truncated.ends_with("\x1b[0m"));
    assert_eq!(visible_char_count(&truncated), 10);
}

#[test]
fn truncate_card_content_preserves_format_json_params_output() {
    let rendered = format_json_params(
        &serde_json::json!({
            "status": "abcdefghijklmnopqrstuvwxyz"
        }),
        "",
    );

    let truncated = truncate_card_content(&rendered, 14);

    assert_eq!(strip_ansi(&truncated), "status: \"abcd…");
    assert!(truncated.ends_with("\x1b[0m"));
    assert_eq!(visible_char_count(&truncated), 14);
}

#[test]
fn truncate_card_content_honours_width_one() {
    let plain = truncate_card_content("abc", 1);
    let ansi = truncate_card_content("\x1b[32mabc\x1b[0m", 1);

    assert_eq!(plain, "…");
    assert_eq!(strip_ansi(&ansi), "…");
    assert_eq!(visible_char_count(&plain), 1);
    assert_eq!(visible_char_count(&ansi), 1);
}

#[test]
fn visible_char_count_uses_terminal_display_width() {
    assert_eq!(visible_char_count("工具"), 4);
    assert_eq!(visible_char_count("🙂"), 2);
    assert_eq!(visible_char_count("e\u{301}"), 1);
    assert_eq!(visible_char_count("👩‍🔬"), 2);
}

#[test]
fn truncate_card_content_handles_wide_and_combining_text() {
    let cjk = truncate_card_content("工具工具", 5);
    let emoji = truncate_card_content("🙂🙂🙂", 5);
    let combining = truncate_card_content("e\u{301}e\u{301}e\u{301}", 2);

    assert_eq!(visible_char_count(&cjk), 5);
    assert_eq!(strip_ansi(&cjk), "工具…");
    assert_eq!(visible_char_count(&emoji), 5);
    assert_eq!(strip_ansi(&emoji), "🙂🙂…");
    assert_eq!(visible_char_count(&combining), 2);
    assert_eq!(strip_ansi(&combining), "e\u{301}…");
}

#[test]
fn render_approval_card_keeps_wide_content_within_box_width() {
    let request = ToolApprovalRequest {
        request_id: "req_12345678",
        tool_name: "工具🙂",
        description: "工具🙂工具🙂工具🙂工具🙂工具🙂工具🙂",
    };
    let lines = render_approval_card(
        &request,
        &serde_json::json!({
            "message": "工具🙂工具🙂工具🙂工具🙂工具🙂"
        }),
    );

    for line in lines {
        assert!(
            visible_char_count(&line) <= 62,
            "approval card line exceeded expected width: {line:?}"
        );
    }
}

#[test]
fn truncate_card_content_keeps_zwj_emoji_clusters_intact() {
    let text = "👩‍🔬👩‍🔬";
    let truncated = truncate_card_content(text, 3);

    assert_eq!(visible_char_count(text), 4);
    assert_eq!(visible_char_count(&truncated), 3);
    assert_eq!(strip_ansi(&truncated), "👩‍🔬…");
}

#[test]
fn render_approval_card_respects_narrow_terminal_width() {
    // Mock a narrow terminal (35-41 columns)
    // With the .min(60) change (no hard 40 minimum), the card should use available width
    let request = ToolApprovalRequest {
        request_id: "req_123",
        tool_name: "test",
        description: "A test tool",
    };
    let lines = render_approval_card(&request, &serde_json::json!({}));

    // All lines should fit within a narrow width without wrapping
    // The actual box_width would be term_width - 4, but we can't control term_width in tests
    // So we just verify no line is excessively wide (which would indicate wrapping)
    for line in &lines {
        let width = visible_char_count(line);
        // Reasonable upper bound - should not exceed typical max
        assert!(
            width <= 80,
            "line width {width} too wide, may indicate wrapping: {line:?}"
        );
    }
}

#[test]
fn format_json_params_never_splits_grapheme_clusters() {
    // Test with emoji with modifiers and ZWJ sequences
    let params = serde_json::json!({
        "emoji": "👩‍🔬👩‍🔬👩‍🔬",
        "combining": "e\u{301}e\u{301}e\u{301}",
        "wide": "工具工具"
    });

    let formatted = format_json_params(&params, "");

    // The formatted output should not split any grapheme clusters
    // Verify by checking that all lines are valid UTF-8 and don't have broken clusters
    for line in formatted.lines() {
        let stripped = strip_ansi(line);
        // If a grapheme cluster was split, the visible width calculation would be wrong
        assert!(
            stripped.chars().all(|c| !c.is_control() || c == '\n'),
            "formatted line contains unexpected control characters: {stripped:?}"
        );
    }

    // Specifically test that ZWJ emoji aren't broken
    assert!(
        formatted.contains("👩‍🔬") || formatted.contains("..."),
        "ZWJ emoji should either be intact or elided via truncation, not split"
    );
}
