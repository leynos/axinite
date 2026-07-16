//! WASM tool wrapper implementing the Tool trait.
//!
//! Uses wasmtime::component::bindgen! to generate typed bindings from the WIT
//! interface, ensuring all host functions are properly registered under the
//! correct `near:agent/host` namespace.
//!
//! Each execution creates a fresh instance (NEAR pattern) to ensure
//! isolation and deterministic behaviour.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use wasmtime::Store;
use wasmtime::component::{HasSelf, Linker};
use wasmtime_wasi::{ResourceTable, WasiCtx, WasiCtxBuilder, WasiCtxView, WasiView};

use crate::context::JobContext;
use crate::safety::LeakDetector;
use crate::secrets::SecretsStore;
use crate::tools::tool::{HostedToolCatalogSource, NativeTool, ToolError, ToolOutput};
use crate::tools::wasm::capabilities::Capabilities;
use crate::tools::wasm::credential_injector::{
    InjectedCredentials, host_matches_pattern, inject_credential,
};
use crate::tools::wasm::error::WasmError;
use crate::tools::wasm::host::{HostState, LogLevel};
use crate::tools::wasm::limits::{ResourceLimits, WasmResourceLimiter};
use crate::tools::wasm::runtime::{EPOCH_TICK_INTERVAL, PreparedModule, WasmToolRuntime};

mod credentials;
mod host_api;
mod http;
pub(crate) mod metadata;
mod store;
mod tool_impl;

#[cfg(test)]
use credentials::resolve_host_credentials;
#[cfg(test)]
use http::extract_host_from_url;
#[cfg(test)]
use store::{ResolvedHostCredential, StoreData};

#[cfg(test)]
mod tests;

// Generate component model bindings from the WIT file.
//
// This creates:
// - `near::agent::host::Host` trait + `add_to_linker()` for the import interface
// - `SandboxedTool` struct with `instantiate()` for the world
// - `exports::near::agent::tool::*` types for the export interface
wasmtime::component::bindgen!({
    path: "wit/tool.wit",
    world: "sandboxed-tool",
    with: {},
});

// Alias the export interface types for convenience.
use exports::near::agent::tool as wit_tool;

/// Configuration needed to refresh an expired OAuth access token.
///
/// Extracted at tool load time from the capabilities file's `auth.oauth` section.
/// Passed into `resolve_host_credentials()` so it can transparently refresh
/// tokens before WASM execution.
#[derive(Debug, Clone)]
pub struct OAuthRefreshConfig {
    /// OAuth token exchange URL (e.g., "https://oauth2.googleapis.com/token").
    pub token_url: String,
    /// OAuth client_id.
    pub client_id: String,
    /// OAuth client_secret (optional, some providers use PKCE without a secret).
    pub client_secret: Option<String>,
    /// Secret name of the access token (e.g., "google_oauth_token").
    /// The refresh token lives at `{secret_name}_refresh_token`.
    pub secret_name: String,
    /// Provider hint stored alongside the refreshed secret.
    pub provider: Option<String>,
}

/// A Tool implementation backed by a WASM component.
///
/// Each call to `execute` creates a fresh instance for isolation.
pub struct WasmToolWrapper {
    /// Runtime for engine access.
    runtime: Arc<WasmToolRuntime>,
    /// Prepared module with compiled component.
    prepared: Arc<PreparedModule>,
    /// Capabilities to grant to this tool.
    capabilities: Capabilities,
    /// Cached description (from PreparedModule or override).
    description: String,
    /// Cached schema (from PreparedModule or override).
    schema: serde_json::Value,
    /// Injected credentials for HTTP requests (e.g., OAuth tokens).
    /// Keys are placeholder names like "GOOGLE_ACCESS_TOKEN".
    credentials: HashMap<String, String>,
    /// Secrets store for resolving host-based credential injection.
    /// Used in execute() to pre-decrypt secrets before WASM runs.
    secrets_store: Option<Arc<dyn SecretsStore + Send + Sync>>,
    /// OAuth refresh configuration for auto-refreshing expired tokens.
    oauth_refresh: Option<OAuthRefreshConfig>,
}

/// Coerce parameter values to match their JSON Schema-declared types.
///
/// LLMs frequently send numeric values as strings (e.g. `"5"` instead of `5`)
/// or booleans as strings (`"true"` instead of `true`). This walks the params
/// object and converts string values where the schema expects a different type.
fn coerce_params_to_schema(
    mut params: serde_json::Value,
    schema: &serde_json::Value,
) -> serde_json::Value {
    let properties = schema.get("properties").and_then(|p| p.as_object());

    let properties = match properties {
        Some(p) => p,
        None => return params,
    };

    let obj = match params.as_object_mut() {
        Some(o) => o,
        None => return params,
    };

    for (key, prop_schema) in properties {
        let declared_type = prop_schema.get("type").and_then(|t| t.as_str());
        let declared_type = match declared_type {
            Some(t) => t,
            None => continue,
        };

        if let Some(current_value) = obj.get_mut(key)
            && let Some(s) = current_value.as_str()
        {
            if declared_type == "string" {
                continue;
            }

            let coerced = match declared_type {
                "number" => s.parse::<f64>().ok().map(serde_json::Value::from),
                "integer" => s.parse::<i64>().ok().map(serde_json::Value::from),
                "boolean" => match s.to_lowercase().as_str() {
                    "true" => Some(serde_json::json!(true)),
                    "false" => Some(serde_json::json!(false)),
                    _ => None,
                },
                _ => None,
            };

            if let Some(new_val) = coerced {
                *current_value = new_val;
            }
        }
    }

    params
}
