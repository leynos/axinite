//! Terminal formatting helpers: termimad skin, help text, and JSON param display.

use lazy_regex::{Regex, regex};
use termimad::MadSkin;
use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

use super::input::SLASH_COMMANDS;

/// A pending tool-use request awaiting user approval.
pub(super) struct ToolApprovalRequest<'a> {
    pub request_id: &'a str,
    pub tool_name: &'a str,
    pub description: &'a str,
}

/// Build a termimad skin with our color scheme.
pub(super) fn make_skin() -> MadSkin {
    let mut skin = MadSkin::default();
    skin.set_headers_fg(termimad::crossterm::style::Color::Yellow);
    skin.bold.set_fg(termimad::crossterm::style::Color::White);
    skin.italic
        .set_fg(termimad::crossterm::style::Color::Magenta);
    skin.inline_code
        .set_fg(termimad::crossterm::style::Color::Green);
    skin.code_block
        .set_fg(termimad::crossterm::style::Color::Green);
    skin.code_block.left_margin = 2;
    skin
}

/// Print the REPL help text to stdout.
pub(super) fn print_help() {
    // Bold white for section headers, bold cyan for commands, dim gray for descriptions
    let h = "\x1b[1m"; // bold (section headers)
    let c = "\x1b[1;36m"; // bold cyan (commands)
    let d = "\x1b[90m"; // dim gray (descriptions)
    let r = "\x1b[0m"; // reset

    // Group commands by category
    let general_cmds = &["/help", "/debug", "/quit", "/exit"];
    let conversation_cmds = &["/undo", "/redo", "/clear", "/compact", "/new", "/interrupt"];

    println!();
    println!("  {h}IronClaw REPL{r}");
    println!();
    println!("  {h}Commands{r}");
    for cmd in SLASH_COMMANDS
        .iter()
        .filter(|c| general_cmds.contains(&c.name))
    {
        println!("  {c}{:<16}{r}  {d}{}{r}", cmd.name, cmd.description);
    }
    println!();
    println!("  {h}Conversation{r}");
    for cmd in SLASH_COMMANDS
        .iter()
        .filter(|c| conversation_cmds.contains(&c.name))
    {
        println!("  {c}{:<16}{r}  {d}{}{r}", cmd.name, cmd.description);
    }
    println!("  {c}esc{r}              {d}stop current operation{r}");
    println!();
    println!("  {h}Approval responses{r}");
    println!("  {c}yes{r} ({c}y{r})         {d}approve tool execution{r}");
    println!("  {c}no{r} ({c}n{r})          {d}deny tool execution{r}");
    println!("  {c}always{r} ({c}a{r})      {d}approve for this session{r}");
    println!();
}

/// Format JSON params as `key: value` lines for the approval card.
pub(super) fn format_json_params(params: &serde_json::Value, indent: &str) -> String {
    match params {
        serde_json::Value::Object(map) => {
            let mut lines = Vec::new();
            for (key, value) in map {
                let val_str = match value {
                    serde_json::Value::String(s) => {
                        let display = if s.chars().count() > 120 {
                            let truncated: String = s.chars().take(120).collect();
                            format!("{truncated}...")
                        } else {
                            s.to_string()
                        };
                        format!("\x1b[32m\"{display}\"\x1b[0m")
                    }
                    other => {
                        let rendered = other.to_string();
                        if rendered.chars().count() > 120 {
                            let truncated: String = rendered.chars().take(120).collect();
                            format!("{truncated}...")
                        } else {
                            rendered
                        }
                    }
                };
                lines.push(format!("{indent}\x1b[36m{key}\x1b[0m: {val_str}"));
            }
            lines.join("\n")
        }
        other => {
            let pretty = serde_json::to_string_pretty(other).unwrap_or_else(|_| other.to_string());
            let truncated = if pretty.chars().count() > 300 {
                let truncated_str: String = pretty.chars().take(300).collect();
                format!("{truncated_str}...")
            } else {
                pretty
            };
            truncated
                .lines()
                .map(|l| format!("{indent}\x1b[90m{l}\x1b[0m"))
                .collect::<Vec<_>>()
                .join("\n")
        }
    }
}

fn ansi_sgr_regex() -> &'static Regex {
    regex!(r"\x1b\[[0-9;]*m")
}

fn visible_char_count(text: &str) -> usize {
    UnicodeWidthStr::width(ansi_sgr_regex().replace_all(text, "").as_ref())
}

fn append_visible_chars(
    text: &str,
    limit: usize,
    visible_count: &mut usize,
    output: &mut String,
) -> bool {
    for grapheme in UnicodeSegmentation::graphemes(text, true) {
        let grapheme_width = UnicodeWidthStr::width(grapheme);
        if grapheme_width > 0 && *visible_count + grapheme_width > limit {
            return false;
        }
        output.push_str(grapheme);
        *visible_count += grapheme_width;
    }
    true
}

/// Scan `text` for ANSI SGR sequences, copying at most `visible_limit` visible
/// characters into a new `String`. Returns the accumulated string and a flag
/// indicating whether an active (non-reset) style sequence was the last one
/// emitted, plus whether additional visible text remained past the limit.
fn truncate_ansi_aware(text: &str, visible_limit: usize) -> (String, bool, bool) {
    let mut truncated = String::new();
    let mut visible_count = 0;
    let mut cursor = 0;
    let mut has_active_style = false;
    let mut was_truncated = false;

    for ansi_match in ansi_sgr_regex().find_iter(text) {
        if visible_count >= visible_limit {
            was_truncated = visible_char_count(&text[cursor..]) > 0;
            break;
        }
        let copied_full_segment = append_visible_chars(
            &text[cursor..ansi_match.start()],
            visible_limit,
            &mut visible_count,
            &mut truncated,
        );
        if visible_count >= visible_limit {
            was_truncated =
                !copied_full_segment || visible_char_count(&text[ansi_match.start()..]) > 0;
            break;
        }
        let ansi_sequence = ansi_match.as_str();
        truncated.push_str(ansi_sequence);
        has_active_style = ansi_sequence != "\x1b[0m";
        cursor = ansi_match.end();
    }

    if visible_count < visible_limit {
        was_truncated = !append_visible_chars(
            &text[cursor..],
            visible_limit,
            &mut visible_count,
            &mut truncated,
        );
    }

    (truncated, has_active_style, was_truncated)
}

/// Truncate content to fit within card width, respecting UTF-8 boundaries.
///
/// Adds "…" if truncated. The returned string will fit within `max_width` characters.
fn truncate_card_content(text: &str, max_width: usize) -> String {
    if max_width == 0 {
        return String::new();
    }
    if visible_char_count(text) <= max_width {
        return text.to_string();
    }
    let visible_limit = max_width.saturating_sub(1);
    let (mut truncated, has_active_style, was_truncated) = truncate_ansi_aware(text, visible_limit);
    debug_assert!(was_truncated || visible_limit == 0);
    truncated.push('…');
    if has_active_style && !truncated.ends_with("\x1b[0m") {
        truncated.push_str("\x1b[0m");
    }
    truncated
}

/// Constructs and returns lines for a tool approval card.
///
/// Draws a bordered box with tool name, description, parameters, and approval options.
/// The returned lines are intended for printing by the caller.
pub(super) fn render_approval_card(
    request: &ToolApprovalRequest<'_>,
    parameters: &serde_json::Value,
) -> Vec<String> {
    let term_width = crossterm::terminal::size()
        .map(|(w, _)| w as usize)
        .unwrap_or(80);
    let box_width = (term_width.saturating_sub(4)).clamp(40, 60);
    let content_width = box_width.saturating_sub(4); // Account for "│ " prefix and padding

    // Short request ID for the bottom border (UTF-8 safe truncation)
    let short_id: String = request.request_id.chars().take(8).collect();

    // Top border: ┌ tool_name requires approval ───
    let top_label = format!(" {} requires approval ", request.tool_name);
    let truncated_label = truncate_card_content(&top_label, box_width.saturating_sub(1));
    let top_fill = box_width.saturating_sub(visible_char_count(&truncated_label) + 1);
    let top_border = format!(
        "\u{250C}\x1b[33m{truncated_label}\x1b[0m{}",
        "\u{2500}".repeat(top_fill)
    );

    // Bottom border: └─ short_id ─────
    let bot_label = format!(" {short_id} ");
    let bot_fill = box_width.saturating_sub(visible_char_count(&bot_label) + 2);
    let bot_border = format!(
        "\u{2514}\u{2500}\x1b[90m{bot_label}\x1b[0m{}",
        "\u{2500}".repeat(bot_fill)
    );

    // Truncate description to fit within card
    let truncated_desc = truncate_card_content(request.description, content_width);

    let mut lines = Vec::new();
    lines.push(String::new()); // blank line
    lines.push(format!("  {top_border}"));
    lines.push(format!("  \u{2502} \x1b[90m{truncated_desc}\x1b[0m"));
    lines.push("  \u{2502}".to_string());

    // Params - truncate each line to fit within the card width
    let param_lines = format_json_params(parameters, "  \u{2502}   ");
    for line in param_lines.lines() {
        lines.push(truncate_card_content(line, box_width));
    }

    lines.push("  \u{2502}".to_string());
    lines.push(
        "  \u{2502} \x1b[32myes\x1b[0m (y) / \x1b[34malways\x1b[0m (a) / \x1b[31mno\x1b[0m (n)"
            .to_string(),
    );
    lines.push(format!("  {bot_border}"));
    lines.push(String::new()); // blank line

    lines
}

#[cfg(test)]
mod tests {
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
}
