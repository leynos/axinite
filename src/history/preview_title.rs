//! Shared conversation preview-title selection logic.

/// Select the best available conversation preview title.
///
/// Fallback order is:
/// 1. SQL-derived preview title
/// 2. `metadata.title`
/// 3. `metadata.routine_name`
pub(crate) fn preview_title_from_metadata(
    metadata: &serde_json::Value,
    sql_title: Option<String>,
) -> Option<String> {
    sql_title
        .or_else(|| {
            metadata
                .get("title")
                .and_then(|value| value.as_str())
                .map(String::from)
        })
        .or_else(|| {
            metadata
                .get("routine_name")
                .and_then(|value| value.as_str())
                .map(String::from)
        })
}

#[cfg(test)]
mod tests {
    use rstest::rstest;
    use serde_json::json;

    use super::preview_title_from_metadata;

    #[rstest]
    #[case(
        Some("First user message".to_string()),
        json!({"title": "Assistant", "routine_name": "daily-standup"}),
        Some("First user message".to_string())
    )]
    #[case(
        None,
        json!({"title": "Assistant", "routine_name": "daily-standup"}),
        Some("Assistant".to_string())
    )]
    #[case(
        None,
        json!({"routine_name": "daily-standup"}),
        Some("daily-standup".to_string())
    )]
    fn preview_title_fallback_matrix(
        #[case] sql_title: Option<String>,
        #[case] metadata: serde_json::Value,
        #[case] expected: Option<String>,
    ) {
        assert_eq!(preview_title_from_metadata(&metadata, sql_title), expected);
    }
}
