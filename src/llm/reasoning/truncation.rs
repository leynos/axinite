//! Truncation of LLM responses at unclosed tool-call XML tags so that useful
//! text before the tag survives the cleaning pipeline (issue #789).

use super::{find_code_regions, is_inside_code};

/// Patterns that indicate tool-call XML in model output.
pub(super) const TOOL_TAG_PATTERNS: &[&str] = &[
    "<tool_call>",
    "<tool_call ",
    "<function_call>",
    "<function_call ",
    "<tool_calls>",
    "<tool_calls ",
    "<|tool_call|>",
    "<|function_call|>",
    "<|tool_calls|>",
];

/// Truncate text at the first **unclosed** tool-call XML tag, preserving content
/// before it.
///
/// Local models (Qwen3, DeepSeek, etc.) often emit `<tool_call>` XML in text
/// responses even when no tools are available. The downstream `clean_response()`
/// → `strip_xml_tag()` pipeline discards everything from an unclosed opening
/// tag onward, which can leave an empty string and trigger the fallback message.
///
/// This function truncates at the first *unclosed* tool tag BEFORE
/// `clean_response()` runs, so the useful text before the tag is preserved.
/// Properly closed tags (e.g. `<tool_call>...</tool_call>`) are left intact for
/// `clean_response()` to strip normally. Tags inside fenced markdown code blocks
/// or inline code spans are ignored. See issue #789.
pub(super) fn truncate_at_tool_tags(text: &str) -> String {
    let code_regions = find_code_regions(text);
    // Use ASCII-only lowercasing so byte offsets stay valid for the original
    // string. Full `to_lowercase()` can change byte lengths for non-ASCII
    // chars (e.g. the Kelvin sign), making positions unreliable.
    let lower = text.to_ascii_lowercase();
    let first_unclosed = TOOL_TAG_PATTERNS
        .iter()
        .filter_map(|p| {
            let mut search_from = 0;
            loop {
                match lower[search_from..].find(p) {
                    Some(offset) => {
                        let pos = search_from + offset;
                        if is_inside_code(pos, &code_regions) {
                            search_from = pos + 1;
                            continue;
                        }
                        // Check if this tag has a matching closing tag after it.
                        // If so, clean_response() can handle it — skip to next.
                        let after_open = pos + p.len();
                        if closing_tag_for(p)
                            .is_some_and(|close| lower[after_open..].contains(close.as_str()))
                        {
                            search_from = after_open;
                            continue;
                        }
                        // Unclosed tag — truncate here
                        return Some(pos);
                    }
                    None => return None,
                }
            }
        })
        .min();
    match first_unclosed {
        Some(pos) => {
            tracing::debug!(
                original_len = text.len(),
                truncated_at = pos,
                "Truncated response at unclosed tool-call XML tag (issue #789)"
            );
            text[..pos].to_string()
        }
        None => text.to_string(),
    }
}

/// Derive the closing tag for a tool-call opening pattern.
///
/// Examples: `<tool_call>` → `</tool_call>`, `<|tool_call|>` → `<|/tool_call|>`.
pub(super) fn closing_tag_for(open_pattern: &str) -> Option<String> {
    if let Some(name) = open_pattern
        .strip_prefix("<|")
        .and_then(|s| s.strip_suffix("|>"))
    {
        // Pipe-delimited: <|tool_call|> → <|/tool_call|>
        Some(format!("<|/{name}|>"))
    } else if let Some(rest) = open_pattern.strip_prefix('<') {
        // Standard XML: <tool_call> or <tool_call  → </tool_call>
        let name = rest.trim_end_matches('>').trim();
        Some(format!("</{name}>"))
    } else {
        None
    }
}
