//! Terminal formatting helpers: termimad skin, help text, and JSON param display.

use termimad::MadSkin;

use super::input::SLASH_COMMANDS;

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

/// Truncate content to fit within card width, respecting UTF-8 boundaries.
///
/// Adds "…" if truncated. The returned string will fit within `max_width` characters.
fn truncate_card_content(text: &str, max_width: usize) -> String {
    if text.chars().count() <= max_width {
        text.to_string()
    } else {
        let truncated: String = text.chars().take(max_width.saturating_sub(1)).collect();
        format!("{}…", truncated)
    }
}

/// Constructs and returns lines for a tool approval card.
///
/// Draws a bordered box with tool name, description, parameters, and approval options.
/// The returned lines are intended for printing by the caller.
pub(super) fn render_approval_card(
    request_id: &str,
    tool_name: &str,
    description: &str,
    parameters: &serde_json::Value,
) -> Vec<String> {
    let term_width = crossterm::terminal::size()
        .map(|(w, _)| w as usize)
        .unwrap_or(80);
    let box_width = (term_width.saturating_sub(4)).clamp(40, 60);
    let content_width = box_width.saturating_sub(4); // Account for "│ " prefix and padding

    // Short request ID for the bottom border (UTF-8 safe truncation)
    let short_id: String = request_id.chars().take(8).collect();

    // Top border: ┌ tool_name requires approval ───
    let top_label = format!(" {tool_name} requires approval ");
    let top_fill = box_width.saturating_sub(top_label.len() + 1);
    let top_border = format!(
        "\u{250C}\x1b[33m{top_label}\x1b[0m{}",
        "\u{2500}".repeat(top_fill)
    );

    // Bottom border: └─ short_id ─────
    let bot_label = format!(" {short_id} ");
    let bot_fill = box_width.saturating_sub(bot_label.len() + 2);
    let bot_border = format!(
        "\u{2514}\u{2500}\x1b[90m{bot_label}\x1b[0m{}",
        "\u{2500}".repeat(bot_fill)
    );

    // Truncate description to fit within card
    let truncated_desc = truncate_card_content(description, content_width);

    let mut lines = Vec::new();
    lines.push(String::new()); // blank line
    lines.push(format!("  {top_border}"));
    lines.push(format!("  \u{2502} \x1b[90m{truncated_desc}\x1b[0m"));
    lines.push("  \u{2502}".to_string());

    // Params - each line is already formatted with prefix, just add them
    let param_lines = format_json_params(parameters, "  \u{2502}   ");
    for line in param_lines.lines() {
        lines.push(line.to_string());
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
