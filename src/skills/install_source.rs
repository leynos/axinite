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

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::{non_blank_raw, trimmed_non_empty};

    #[rstest]
    #[case::none(None)]
    #[case::empty(Some(""))]
    #[case::spaces(Some("  \t\n  "))]
    fn non_blank_raw_rejects_missing_or_whitespace(#[case] value: Option<&str>) {
        assert_eq!(non_blank_raw(value), None);
    }

    #[rstest]
    #[case::plain("content")]
    #[case::surrounding_whitespace("  content\n")]
    fn non_blank_raw_preserves_original_non_blank_value(#[case] value: &str) {
        assert_eq!(non_blank_raw(Some(value)), Some(value));
    }

    #[rstest]
    #[case::none(None)]
    #[case::empty(Some(""))]
    #[case::spaces(Some("  \t\n  "))]
    fn trimmed_non_empty_rejects_missing_or_whitespace(#[case] value: Option<&str>) {
        assert_eq!(trimmed_non_empty(value), None);
    }

    #[rstest]
    #[case::plain("deploy-docs", "deploy-docs")]
    #[case::surrounding_whitespace("  deploy-docs\n", "deploy-docs")]
    fn trimmed_non_empty_returns_trimmed_non_blank_value(
        #[case] value: &str,
        #[case] expected: &str,
    ) {
        assert_eq!(trimmed_non_empty(Some(value)), Some(expected));
    }
}
