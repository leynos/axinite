//! Response cleaning pipeline: compiled tag regexes and the `clean_response`
//! entry point that strips model-internal reasoning and tool-call tags.

use std::sync::LazyLock;

use regex::Regex;

use super::{
    extract_final_content, find_code_regions, strip_pipe_reasoning_tags, strip_pipe_tag,
    strip_thinking_tags_regex, strip_xml_tag,
};

/// Compile a static tag-stripping regex, logging and yielding `None` when the
/// pattern fails to compile so callers can skip that stripping stage instead
/// of panicking at first use.
fn compile_tag_regex(name: &str, pattern: &str) -> Option<Regex> {
    match Regex::new(pattern) {
        Ok(re) => Some(re),
        Err(error) => {
            tracing::error!("failed to compile {name} regex: {error}");
            None
        }
    }
}

/// Quick-check: bail early if no reasoning/final tags are present at all.
pub(super) static QUICK_TAG_RE: LazyLock<Option<Regex>> = LazyLock::new(|| {
    compile_tag_regex(
        "QUICK_TAG_RE",
        r"(?i)<\s*/?\s*(?:think(?:ing)?|thought|thoughts|antthinking|reasoning|reflection|scratchpad|inner_monologue|final)\b",
    )
});

/// Matches thinking/reasoning open and close tags. Capture group 1 is "/" for close tags.
/// Whitespace-tolerant, case-insensitive, attribute-aware.
pub(super) static THINKING_TAG_RE: LazyLock<Option<Regex>> = LazyLock::new(|| {
    compile_tag_regex(
        "THINKING_TAG_RE",
        r"(?i)<\s*(/?)\s*(?:think(?:ing)?|thought|thoughts|antthinking|reasoning|reflection|scratchpad|inner_monologue)\b[^<>]*>",
    )
});

/// Matches `<final>` / `</final>` tags. Capture group 1 is "/" for close tags.
pub(super) static FINAL_TAG_RE: LazyLock<Option<Regex>> =
    LazyLock::new(|| compile_tag_regex("FINAL_TAG_RE", r"(?i)<\s*(/?)\s*final\b[^<>]*>"));

/// Matches pipe-delimited reasoning tags: `<|think|>...<|/think|>` etc.
pub(super) static PIPE_REASONING_TAG_RE: LazyLock<Option<Regex>> = LazyLock::new(|| {
    compile_tag_regex(
        "PIPE_REASONING_TAG_RE",
        r"(?i)<\|(/?)\s*(?:think(?:ing)?|thought|thoughts|antthinking|reasoning|reflection|scratchpad|inner_monologue)\|>",
    )
});

/// Tool-related tags stripped with simple string matching (no code-awareness needed).
const TOOL_TAGS: &[&str] = &["tool_call", "function_call", "tool_calls"];

/// Clean up LLM response by stripping model-internal tags and reasoning patterns.
///
/// Some models (GLM-4.7, etc.) emit XML-tagged internal state like
/// `<tool_call>tool_list</tool_call>` or `<|tool_call|>` in the content field
/// instead of using the standard OpenAI tool_calls array. We strip all of
/// these before the response reaches channels/users.
///
/// Pipeline:
/// 1. Quick-check — bail if no reasoning/final tags
/// 2. Build code regions (fenced blocks + inline backticks)
/// 3. Strip thinking tags (regex, code-aware, strict mode for unclosed)
/// 4. If `<final>` tags present: extract only `<final>` content
///    Else: use the thinking-stripped text as-is
/// 5. Strip pipe-delimited reasoning tags (code-aware)
/// 6. Strip tool tags (string matching — no code-awareness needed)
/// 7. Collapse triple+ newlines, trim
pub(super) fn clean_response(text: &str) -> String {
    // 1. Quick-check
    let has_tags = QUICK_TAG_RE.as_ref().is_some_and(|re| re.is_match(text));
    let mut result = if !has_tags {
        text.to_string()
    } else {
        // 2 + 3. Build code regions, strip thinking tags
        let code_regions = find_code_regions(text);
        let after_thinking = strip_thinking_tags_regex(text, &code_regions);

        // 4. If <final> tags present, extract only their content
        let has_final = FINAL_TAG_RE
            .as_ref()
            .is_some_and(|re| re.is_match(&after_thinking));
        if has_final {
            let fresh_regions = find_code_regions(&after_thinking);
            extract_final_content(&after_thinking, &fresh_regions).unwrap_or(after_thinking)
        } else {
            after_thinking
        }
    };

    // 5. Strip pipe-delimited reasoning tags (code-aware)
    result = strip_pipe_reasoning_tags(&result);

    // 6. Strip tool tags (string matching, not code-aware)
    for tag in TOOL_TAGS {
        result = strip_xml_tag(&result, tag);
        result = strip_pipe_tag(&result, tag);
    }

    // 6b. Strip bracket-format inline tool calls: [Called tool `name` with arguments: {...}]
    result = strip_bracket_tool_calls(&result);

    // 7. Collapse triple+ newlines, trim
    collapse_newlines(&result)
}

/// Strip bracket-format inline tool calls produced by `flatten_tool_messages`.
///
/// Removes patterns like `[Called tool `name` with arguments: {...}]` from text
/// so the user doesn't see raw tool call syntax when the model echoes it back.
fn strip_bracket_tool_calls(text: &str) -> String {
    let mut result = String::with_capacity(text.len());
    let mut remaining = text;
    while let Some(start) = remaining.find("[Called tool `") {
        result.push_str(&remaining[..start]);
        let after = &remaining[start..];
        // Find the closing "]" for this bracket expression
        if let Some(end) = after.find("]\n").map(|i| i + 2).or_else(|| {
            // If it's at the end of the string, just find "]"
            after.rfind(']').map(|i| i + 1)
        }) {
            remaining = &after[end..];
        } else {
            // Malformed — keep the rest
            result.push_str(after);
            return result;
        }
    }
    result.push_str(remaining);
    result
}

/// Collapse triple+ newlines to double, then trim.
fn collapse_newlines(text: &str) -> String {
    let mut result = text.to_string();
    while result.contains("\n\n\n") {
        result = result.replace("\n\n\n", "\n\n");
    }
    result.trim().to_string()
}
