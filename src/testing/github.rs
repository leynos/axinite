//! Shared test helpers for GitHub WASM tool assertions.
//!
//! Provides assertions for validating the real GitHub tool schema
//! in integration tests across the codebase.

/// Assert that the given parameters match the expected real GitHub schema.
///
/// This helper centralizes the schema expectations for the GitHub WASM tool
/// to avoid duplication between registry and loader tests.
///
/// # Example
///
/// ```rust,ignore
/// use ironclaw::testing::github::assert_real_github_schema;
///
/// let definition = registry
///     .tool_definitions()
///     .await
///     .into_iter()
///     .find(|d| d.name == "github")
///     .expect("github definition should be registered");
///
/// assert_real_github_schema(&definition.parameters);
/// ```
pub fn assert_real_github_schema(parameters: &serde_json::Value) {
    assert_eq!(parameters["type"], serde_json::json!("object"));
    // A missing or non-array value fails the assertion rather than panicking
    // separately, so the diagnostic message still shows the offending JSON.
    let action_enum = parameters["properties"]["action"]["enum"].as_array();
    assert!(
        action_enum.is_some_and(|values| values.iter().any(|value| value == "get_repo")),
        "expected 'get_repo' in action enum array: {}",
        parameters["properties"]["action"]["enum"]
    );
    let required = parameters["required"].as_array();
    assert!(
        required.is_some_and(|values| values.iter().any(|value| value == "action")),
        "expected required array with action field in schema: {}",
        parameters
    );
}
