//! Shared helpers for interpreting skill install source fields.

/// Return raw text when a field has non-whitespace content.
///
/// Inline `SKILL.md` content must keep its original bytes, but whitespace-only
/// values are not valid install sources.
pub(crate) fn non_blank_raw(value: Option<&str>) -> Option<&str> {
    value.filter(|value| !value.trim().is_empty())
}

/// Return trimmed text when a source identifier has non-whitespace content.
pub(crate) fn trimmed_non_empty(value: Option<&str>) -> Option<&str> {
    value.map(str::trim).filter(|value| !value.is_empty())
}
