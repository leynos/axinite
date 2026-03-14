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
