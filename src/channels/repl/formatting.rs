//! Terminal formatting helpers: termimad skin, help text, and JSON param display.

use termimad::MadSkin;

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

    println!();
    println!("  {h}IronClaw REPL{r}");
    println!();
    println!("  {h}Commands{r}");
    println!("  {c}/help{r}              {d}show this help{r}");
    println!("  {c}/debug{r}             {d}toggle verbose output{r}");
    println!("  {c}/quit{r} {c}/exit{r}        {d}exit the repl{r}");
    println!();
    println!("  {h}Conversation{r}");
    println!("  {c}/undo{r}              {d}undo the last turn{r}");
    println!("  {c}/redo{r}              {d}redo an undone turn{r}");
    println!("  {c}/clear{r}             {d}clear conversation{r}");
    println!("  {c}/compact{r}           {d}compact context window{r}");
    println!("  {c}/new{r}               {d}new conversation thread{r}");
    println!("  {c}/interrupt{r}         {d}stop current operation{r}");
    println!("  {c}esc{r}                {d}stop current operation{r}");
    println!();
    println!("  {h}Approval responses{r}");
    println!("  {c}yes{r} ({c}y{r})            {d}approve tool execution{r}");
    println!("  {c}no{r} ({c}n{r})             {d}deny tool execution{r}");
    println!("  {c}always{r} ({c}a{r})         {d}approve for this session{r}");
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
                        let display = if s.len() > 120 { &s[..120] } else { s };
                        format!("\x1b[32m\"{display}\"\x1b[0m")
                    }
                    other => {
                        let rendered = other.to_string();
                        if rendered.len() > 120 {
                            format!("{}...", &rendered[..120])
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
            let truncated = if pretty.len() > 300 {
                format!("{}...", &pretty[..300])
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
