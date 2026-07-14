//! Helpers for deciding whether a heartbeat checklist has actionable content.

/// Check if heartbeat content is effectively empty.
///
/// Returns true if the content contains only:
/// - Whitespace
/// - Markdown headers (lines starting with #)
/// - HTML comments (`<!-- ... -->`)
/// - Empty list items (`- [ ]`, `- [x]`, `-`, `*`)
///
/// This skips the LLM call when the user hasn't added real tasks yet,
/// saving API costs.
pub(super) fn is_effectively_empty(content: &str) -> bool {
    let without_comments = strip_html_comments(content);

    without_comments.lines().all(|line| {
        let trimmed = line.trim();
        trimmed.is_empty()
            || trimmed.starts_with('#')
            || trimmed == "- [ ]"
            || trimmed == "- [x]"
            || trimmed == "-"
            || trimmed == "*"
    })
}

/// Remove HTML comments from content.
pub(super) fn strip_html_comments(content: &str) -> String {
    let mut result = String::with_capacity(content.len());
    let mut rest = content;
    while let Some(start) = rest.find("<!--") {
        result.push_str(&rest[..start]);
        match rest[start..].find("-->") {
            Some(end) => rest = &rest[start + end + 3..],
            None => return result, // unclosed comment, treat rest as comment
        }
    }
    result.push_str(rest);
    result
}
