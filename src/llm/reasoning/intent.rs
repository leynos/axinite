//! Detection of tool-call intent and silent replies in LLM response text,
//! with prose/code separation so code samples never trigger false positives.

use super::SILENT_REPLY_TOKEN;

/// Detect when an LLM response expresses intent to call a tool without
/// actually issuing tool calls. Returns `true` if the text contains phrases
/// like "Let me search …" or "I'll fetch …" outside of fenced/indented code blocks.
///
/// Exclusion phrases (e.g. "let me explain") are checked first to avoid
/// false positives on conversational language.
pub fn llm_signals_tool_intent(response: &str) -> bool {
    // Extract only non-code lines with quoted strings removed
    let text = strip_code_blocks(response);
    let lower = text.to_lowercase();

    // Exclusion phrases — if any appear, bail out immediately
    const EXCLUSIONS: &[&str] = &[
        "let me explain",
        "let me know",
        "let me think",
        "let me summarize",
        "let me clarify",
        "let me describe",
        "let me help",
        "let me understand",
        "let me break",
        "let me outline",
        "let me walk you",
        "let me provide",
        "let me suggest",
        "let me elaborate",
        "let me start by",
    ];
    if EXCLUSIONS.iter().any(|e| lower.contains(e)) {
        return false;
    }

    const PREFIXES: &[&str] = &["let me ", "i'll ", "i will ", "i'm going to "];
    const ACTION_VERBS: &[&str] = &[
        "search",
        "look up",
        "check",
        "fetch",
        "find",
        "read the",
        "write the",
        "create",
        "run the",
        "execute",
        "query",
        "retrieve",
        "add it",
        "add the",
        "add this",
        "add that",
        "update the",
        "delete",
        "remove the",
        "look into",
    ];

    for prefix in PREFIXES {
        for (i, _) in lower.match_indices(prefix) {
            let after = &lower[i + prefix.len()..];
            for verb in ACTION_VERBS {
                if after.starts_with(verb) || after.contains(&format!(" {verb}")) {
                    return true;
                }
            }
        }
    }

    false
}

/// Strip fenced code blocks (``` ... ```), indented code lines (4+ spaces / tab),
/// and double-quoted strings so that tool-intent detection only fires on prose.
fn strip_code_blocks(text: &str) -> String {
    let mut result = String::new();
    let mut in_fence = false;

    for line in text.lines() {
        let trimmed = line.trim_start();
        if trimmed.starts_with("```") {
            in_fence = !in_fence;
            continue;
        }
        if in_fence {
            continue;
        }
        // Skip indented code lines (4+ spaces or tab)
        if line.starts_with("    ") || line.starts_with('\t') {
            continue;
        }
        // Strip double-quoted strings to avoid matching intent phrases inside quotes
        let stripped = strip_quoted_strings(line);
        result.push_str(&stripped);
        result.push('\n');
    }
    result
}

/// Remove double-quoted string literals from a line.
fn strip_quoted_strings(line: &str) -> String {
    let mut result = String::with_capacity(line.len());
    let mut in_quote = false;
    let mut prev = '\0';
    for ch in line.chars() {
        if ch == '"' && prev != '\\' {
            in_quote = !in_quote;
            continue;
        }
        if !in_quote {
            result.push(ch);
        }
        prev = ch;
    }
    result
}

/// Check if a response is a silent reply (the agent has nothing to say).
///
/// Returns true if the trimmed text is exactly the silent reply token or
/// contains only the token surrounded by whitespace/punctuation.
pub fn is_silent_reply(text: &str) -> bool {
    let trimmed = text.trim();
    trimmed == SILENT_REPLY_TOKEN
        || trimmed.starts_with(SILENT_REPLY_TOKEN)
            && trimmed.len() <= SILENT_REPLY_TOKEN.len() + 4
            && trimmed[SILENT_REPLY_TOKEN.len()..]
                .chars()
                .all(|c| c.is_whitespace() || c.is_ascii_punctuation())
}
