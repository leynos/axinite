//! Shared test helpers for GitHub WASM tool assertions.
//!
//! Provides assertions for validating the real GitHub tool schema
//! in integration tests across the codebase.

/// Assert that the given parameters match the expected real GitHub schema.
///
/// This helper centralises the schema expectations for the GitHub WASM tool
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
    assert_eq!(
        parameters["properties"]["action"]["enum"][0],
        serde_json::json!("get_repo")
    );
    assert!(
        parameters["required"]
            .as_array()
            .expect("required array")
            .iter()
            .any(|value| value == "action"),
        "expected required action field in schema: {}",
        parameters
    );
}
