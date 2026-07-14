//! Secret masking and redaction helpers.
//!
//! Produces safe previews of matched secrets and applies `[REDACTED]`
//! replacements to scanned content.

use std::ops::Range;

/// Mask a secret for safe display.
///
/// Shows first 4 and last 4 characters, masks the middle.
pub(super) fn mask_secret(secret: &str) -> String {
    let len = secret.len();
    if len <= 8 {
        return "*".repeat(len);
    }

    let prefix: String = secret.chars().take(4).collect();
    let suffix: String = secret.chars().skip(len - 4).collect();
    let middle_len = len - 8;
    format!("{}{}{}", prefix, "*".repeat(middle_len.min(8)), suffix)
}

/// Apply redaction ranges to content.
pub(super) fn apply_redactions(content: &str, ranges: &[Range<usize>]) -> String {
    if ranges.is_empty() {
        return content.to_string();
    }

    let mut result = String::with_capacity(content.len());
    let mut last_end = 0;

    for range in ranges {
        if range.start > last_end {
            result.push_str(&content[last_end..range.start]);
        }
        result.push_str("[REDACTED]");
        last_end = range.end;
    }

    if last_end < content.len() {
        result.push_str(&content[last_end..]);
    }

    result
}
