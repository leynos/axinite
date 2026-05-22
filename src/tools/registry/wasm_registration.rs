//! WASM tool registration request and error types used by the tool registry.

use std::sync::Arc;

use crate::secrets::SecretsStore;
use crate::tools::wasm::{
    Capabilities, OAuthRefreshConfig, ResourceLimits, WasmError, WasmStorageError, WasmToolRuntime,
};

/// Error when registering a WASM tool from storage.
#[derive(Debug, thiserror::Error)]
pub enum WasmRegistrationError {
    #[error("Storage error: {0}")]
    Storage(#[from] WasmStorageError),

    #[error("WASM error: {0}")]
    Wasm(#[from] WasmError),
}

/// Configuration for registering a WASM tool.
pub struct WasmToolRegistration<'a> {
    /// Unique name for the tool.
    pub name: &'a str,
    /// Raw WASM component bytes.
    pub wasm_bytes: &'a [u8],
    /// WASM runtime for compilation and execution.
    pub runtime: &'a Arc<WasmToolRuntime>,
    /// Security capabilities to grant the tool.
    pub capabilities: Capabilities,
    /// Optional resource limits (uses defaults if None).
    pub limits: Option<ResourceLimits>,
    /// Optional description override.
    pub description: Option<&'a str>,
    /// Optional parameter schema override.
    pub schema: Option<serde_json::Value>,
    /// Secrets store for credential injection at request time.
    pub secrets_store: Option<Arc<dyn SecretsStore + Send + Sync>>,
    /// OAuth refresh configuration for auto-refreshing expired tokens.
    pub oauth_refresh: Option<OAuthRefreshConfig>,
}

/// Trim a caller-supplied description and reject blank values.
///
/// This `pub(super)` helper returns `Some(&str)` with a trimmed slice into the
/// original input when the result is non-empty, or `None` when the input is
/// empty or contains only whitespace.
pub(super) fn normalized_description(description: &str) -> Option<&str> {
    let trimmed = description.trim();
    (!trimmed.is_empty()).then_some(trimmed)
}

#[cfg(test)]
mod tests {
    //! Unit tests for WASM registration helper logic, including description
    //! normalization for storage-backed registration.

    use rstest::rstest;

    use super::normalized_description;

    #[rstest]
    #[case("", None)]
    #[case("   \n\t  ", None)]
    #[case("  Useful WASM tool  ", Some("Useful WASM tool"))]
    fn normalized_description_trims_and_rejects_blank_input(
        #[case] description: &str,
        #[case] expected: Option<&str>,
    ) {
        assert_eq!(normalized_description(description), expected);
    }
}
