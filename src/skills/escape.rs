//! Content-escape helpers for safe XML-attribute and prompt-skill injection.

use regex::Regex;

/// Escape a string for safe inclusion in XML attributes.
/// Prevents attribute injection attacks via skill name/version fields.
pub fn escape_xml_attr(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

/// Escape prompt content to prevent tag breakout from `<skill>` delimiters.
///
/// Neutralizes both opening (`<skill`) and closing (`</skill`) tags using a
/// case-insensitive regex that catches mixed case, optional whitespace, and
/// null bytes. Opening tags are escaped to prevent injecting fake skill blocks
/// with elevated trust attributes. The `<` is replaced with `&lt;`.
pub fn escape_skill_content(content: &str) -> String {
    static SKILL_TAG_RE: std::sync::LazyLock<Regex> = std::sync::LazyLock::new(|| {
        // Match `<` followed by optional `/`, optional whitespace/control chars,
        // then `skill` (case-insensitive). Catches both opening and closing tags:
        // `<skill`, `</skill`, `< skill`, `</\0skill`, `<SKILL`, etc.
        Regex::new(r"(?i)</?[\s\x00]*skill").unwrap()
    });

    SKILL_TAG_RE
        .replace_all(content, |caps: &regex::Captures| {
            // Replace leading `<` with `&lt;` to neutralize the tag
            let matched = caps.get(0).unwrap().as_str();
            format!("&lt;{}", &matched[1..])
        })
        .into_owned()
}

/// Normalize line endings to LF before hashing to ensure cross-platform consistency.
pub fn normalize_line_endings(content: &str) -> String {
    content.replace("\r\n", "\n").replace('\r', "\n")
}
