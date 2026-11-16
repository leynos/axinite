//! Single-line output previews and internal tool-call marker removal.
//!
//! Collapses tool output for display and strips provider-level
//! `[Called tool ...]` / `[Tool ... returned: ...]` markers that can leak into
//! user-visible responses.

/// Maximum characters for tool output preview.
pub(crate) const PREVIEW_MAX_CHARS: usize = 1024;

/// Collapse a tool output string into a single-line preview for display.
pub(crate) fn truncate_for_preview(output: &str, max_chars: usize) -> String {
    if max_chars == 0 {
        return String::new();
    }
    let collapsed = output.split_whitespace().collect::<Vec<_>>().join(" ");
    let total = collapsed.chars().count();
    if total <= max_chars {
        return collapsed;
    }
    let mut truncated = String::with_capacity(max_chars + 3);
    truncated.extend(collapsed.chars().take(max_chars));
    truncated.push_str("...");
    truncated
}

/// Strip internal `[Called tool ...]` and `[Tool ... returned: ...]` markers
/// from a response string. These markers are inserted by provider-level message
/// flattening (e.g. NEAR AI) and can leak into the user-visible response when
/// the LLM echoes them back.
pub(super) fn strip_internal_tool_call_text(text: &str) -> String {
    // Remove lines that are purely internal tool-call markers.
    // Pattern: lines matching `[Called tool <name>(...)]` or `[Tool <name> returned: ...]`
    let result = text
        .lines()
        .filter(|line| {
            let trimmed = line.trim();
            !((trimmed.starts_with("[Called tool ") && trimmed.ends_with(']'))
                || (trimmed.starts_with("[Tool ")
                    && trimmed.contains(" returned:")
                    && trimmed.ends_with(']')))
        })
        .fold(String::new(), |mut acc, s| {
            if !acc.is_empty() {
                acc.push('\n');
            }
            acc.push_str(s);
            acc
        });

    let result = result.trim();
    if result.is_empty() {
        "I wasn't able to complete that request. Could you try rephrasing or providing more details?".to_string()
    } else {
        result.to_string()
    }
}
