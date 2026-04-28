//! WASM tool loading and registration.
//!
//! This module owns the [`ToolRegistry`] struct and its associated WASM
//! registration methods. It is responsible for:
//!
//! - Compiling and preparing raw WASM bytes via [`WasmToolRuntime`].
//! - Recovering or overriding guest-exported metadata (description and
//!   parameter schema) when stored overrides are absent.
//! - Attaching runtime concerns such as secrets injection and OAuth
//!   refresh configuration.
//! - Registering tools from persistent storage by fetching the stored
//!   record and binary, normalizing metadata via [`schema::normalized_schema`],
//!   and delegating to [`ToolRegistry::register_wasm`].

use std::collections::HashMap;
use std::sync::Arc;

use tokio::sync::RwLock;

use crate::llm::ToolDefinition;
use crate::secrets::{CredentialMapping, SecretsStore};
use crate::tools::rate_limiter::RateLimiter;
use crate::tools::tool::Tool;
use crate::tools::wasm::{
    Capabilities, OAuthRefreshConfig, ResourceLimits, SharedCredentialRegistry, ToolKey, WasmError,
    WasmStorageError, WasmToolRuntime, WasmToolStore,
};

use super::{
    PROTECTED_TOOL_NAMES, is_protected_tool_name, schema::normalized_schema,
    wasm_preparation::prepare_wasm_tool,
};

pub struct WasmFromStorageRegistration<'a> {
    pub store: &'a dyn WasmToolStore,
    pub runtime: &'a Arc<WasmToolRuntime>,
    pub user_id: &'a str,
    pub name: &'a str,
}

/// Registry of available tools.
pub struct ToolRegistry {
    pub(super) tools: RwLock<HashMap<String, Arc<dyn Tool>>>,
    /// Tracks which names were registered as built-in (protected from shadowing).
    pub(super) builtin_names: RwLock<std::collections::HashSet<String>>,
    /// Shared credential registry populated by WASM tools, consumed by HTTP tool.
    pub(super) credential_registry: Option<Arc<SharedCredentialRegistry>>,
    /// Secrets store for credential injection (shared with HTTP tool).
    pub(super) secrets_store: Option<Arc<dyn SecretsStore + Send + Sync>>,
    /// Shared rate limiter for built-in tool invocations.
    rate_limiter: RateLimiter,
    /// Reference to the message tool for setting context per-turn.
    pub(super) message_tool: RwLock<Option<Arc<crate::tools::builtin::MessageTool>>>,
}

impl ToolRegistry {
    fn sync_write_lock<T>(lock: &RwLock<T>) -> tokio::sync::RwLockWriteGuard<'_, T> {
        loop {
            if let Ok(guard) = lock.try_write() {
                return guard;
            }
            std::thread::yield_now();
        }
    }

    /// Whether a tool name belongs to the protected built-in namespace.
    pub fn is_protected_tool_name(name: &str) -> bool {
        is_protected_tool_name(name)
    }

    /// Create a new empty registry.
    pub fn new() -> Self {
        Self {
            tools: RwLock::new(HashMap::new()),
            builtin_names: RwLock::new(std::collections::HashSet::new()),
            credential_registry: None,
            secrets_store: None,
            rate_limiter: RateLimiter::new(),
            message_tool: RwLock::new(None),
        }
    }

    /// Create a registry with credential injection support.
    pub fn with_credentials(
        mut self,
        credential_registry: Arc<SharedCredentialRegistry>,
        secrets_store: Arc<dyn SecretsStore + Send + Sync>,
    ) -> Self {
        self.credential_registry = Some(credential_registry);
        self.secrets_store = Some(secrets_store);
        self
    }

    /// Get a reference to the shared credential registry.
    pub fn credential_registry(&self) -> Option<&Arc<SharedCredentialRegistry>> {
        self.credential_registry.as_ref()
    }

    /// Get the shared rate limiter for checking built-in tool limits.
    pub fn rate_limiter(&self) -> &RateLimiter {
        &self.rate_limiter
    }

    /// Register a tool. Rejects dynamic tools that try to shadow a built-in name.
    pub async fn register(&self, tool: Arc<dyn Tool>) -> bool {
        let name = tool.name().to_string();
        if Self::is_protected_tool_name(&name) {
            tracing::warn!(
                tool = %name,
                "Rejected tool registration: protected tool names cannot be dynamically registered"
            );
            return false;
        }
        if self.builtin_names.read().await.contains(&name) {
            tracing::warn!(
                tool = %name,
                "Rejected tool registration: would shadow a built-in tool"
            );
            return false;
        }
        self.tools.write().await.insert(name.clone(), tool);
        tracing::trace!("Registered tool: {}", name);
        true
    }

    /// Register a tool (sync version for startup, marks as built-in).
    pub fn register_sync(&self, tool: Arc<dyn Tool>) {
        let name = tool.name().to_string();
        let mut tools = Self::sync_write_lock(&self.tools);
        tools.insert(name.clone(), tool);
        if PROTECTED_TOOL_NAMES.contains(&name.as_str()) {
            let mut builtins = Self::sync_write_lock(&self.builtin_names);
            builtins.insert(name.clone());
        }
        tracing::debug!("Registered tool: {}", name);
    }

    /// Unregister a tool.
    pub async fn unregister(&self, name: &str) -> Option<Arc<dyn Tool>> {
        self.tools.write().await.remove(name)
    }

    /// Get a tool by name.
    pub async fn get(&self, name: &str) -> Option<Arc<dyn Tool>> {
        let tools = self.tools.read().await;
        tools.get(name).map(Arc::clone)
    }

    /// Check if a tool exists.
    pub async fn has(&self, name: &str) -> bool {
        self.tools.read().await.contains_key(name)
    }

    /// List all tool names.
    pub async fn list(&self) -> Vec<String> {
        self.tools.read().await.keys().cloned().collect()
    }

    /// Retain only tools whose names are in the given allowlist.
    ///
    /// If `names` is empty, this is a no-op (all tools are kept).
    pub async fn retain_only(&self, names: &[&str]) {
        if names.is_empty() {
            return;
        }
        let names_set: std::collections::HashSet<&str> = names.iter().copied().collect();
        let mut tools = self.tools.write().await;
        tools.retain(|k, _| names_set.contains(k.as_str()));
    }

    /// Get the number of registered tools.
    pub fn count(&self) -> usize {
        self.tools.try_read().map(|t| t.len()).unwrap_or(0)
    }

    /// Get all tools.
    pub async fn all(&self) -> Vec<Arc<dyn Tool>> {
        self.tools.read().await.values().cloned().collect()
    }

    /// Get tool definitions for LLM function calling.
    pub async fn tool_definitions(&self) -> Vec<ToolDefinition> {
        let mut defs: Vec<ToolDefinition> = self
            .tools
            .read()
            .await
            .values()
            .map(|tool| ToolDefinition {
                name: tool.name().to_string(),
                description: tool.description().to_string(),
                parameters: tool.parameters_schema(),
            })
            .collect();
        defs.sort_unstable_by(|a, b| a.name.cmp(&b.name));
        defs
    }

    /// Get tool definitions for specific tools.
    pub async fn tool_definitions_for(&self, names: &[&str]) -> Vec<ToolDefinition> {
        let tools = self.tools.read().await;
        names
            .iter()
            .filter_map(|name| {
                tools.get(*name).map(|tool| ToolDefinition {
                    name: tool.name().to_string(),
                    description: tool.description().to_string(),
                    parameters: tool.parameters_schema(),
                })
            })
            .collect()
    }

    /// Register a WASM tool from bytes.
    ///
    /// This validates and compiles the WASM component, then registers it as a tool.
    /// The tool will be executed in a sandboxed environment with the given capabilities.
    ///
    /// # Example
    ///
    /// ```ignore
    /// let runtime = Arc::new(WasmToolRuntime::new(WasmRuntimeConfig::default())?);
    /// let wasm_bytes = std::fs::read("my_tool.wasm")?;
    ///
    /// registry.register_wasm(WasmToolRegistration {
    ///     name: "my_tool",
    ///     wasm_bytes: &wasm_bytes,
    ///     runtime: &runtime,
    ///     description: Some("My custom tool description"),
    ///     ..Default::default()
    /// }).await?;
    /// ```
    pub async fn register_wasm(&self, reg: WasmToolRegistration<'_>) -> Result<(), WasmError> {
        let name = reg.name;
        let prepared = prepare_wasm_tool(reg).await?;

        let registered = self.register(Arc::new(prepared.wrapper)).await;
        if !registered {
            return Err(WasmError::ConfigError(
                "tool registration rejected".to_string(),
            ));
        }

        self.persist_credential_mappings(name, prepared.credential_mappings);
        tracing::debug!(name, "Registered WASM tool");
        Ok(())
    }

    fn persist_credential_mappings(&self, name: &str, credential_mappings: Vec<CredentialMapping>) {
        if let Some(cr) = &self.credential_registry
            && !credential_mappings.is_empty()
        {
            let count = credential_mappings.len();
            cr.add_mappings(credential_mappings);
            tracing::debug!(
                name,
                credential_count = count,
                "Added credential mappings from WASM tool"
            );
        }
    }

    /// Register a WASM tool from database storage.
    ///
    /// Loads the WASM binary with integrity verification and configures capabilities.
    ///
    /// # Example
    ///
    /// ```ignore
    /// let store = PostgresWasmToolStore::new(pool);
    /// let runtime = Arc::new(WasmToolRuntime::new(WasmRuntimeConfig::default())?);
    ///
    /// registry
    ///     .register_wasm_from_storage(WasmFromStorageRegistration {
    ///         store: &store,
    ///         runtime: &runtime,
    ///         user_id: "user_123",
    ///         name: "my_tool",
    ///     })
    ///     .await?;
    /// ```
    pub async fn register_wasm_from_storage(
        &self,
        req: WasmFromStorageRegistration<'_>,
    ) -> Result<(), WasmRegistrationError> {
        let WasmFromStorageRegistration {
            store,
            runtime,
            user_id,
            name,
        } = req;
        // Load tool with integrity verification
        let tool_with_binary = store
            .get_with_binary(ToolKey { user_id, name })
            .await
            .map_err(WasmRegistrationError::Storage)?;

        // Load capabilities
        let stored_caps = store
            .get_capabilities(tool_with_binary.tool.id)
            .await
            .map_err(WasmRegistrationError::Storage)?;

        let capabilities = stored_caps.map(|c| c.to_capabilities()).unwrap_or_default();

        let description = normalized_description(&tool_with_binary.tool.description);
        let schema = normalized_schema(tool_with_binary.tool.parameters_schema.clone());

        self.register_wasm(WasmToolRegistration {
            name: &tool_with_binary.tool.name,
            wasm_bytes: &tool_with_binary.wasm_binary,
            runtime,
            capabilities,
            limits: None,
            description,
            schema,
            secrets_store: self.secrets_store.clone(),
            oauth_refresh: None,
        })
        .await
        .map_err(WasmRegistrationError::Wasm)?;

        tracing::debug!(
            name = tool_with_binary.tool.name,
            user_id = user_id,
            trust_level = %tool_with_binary.tool.trust_level,
            "Registered WASM tool from storage"
        );

        Ok(())
    }
}

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

impl Default for ToolRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Debug for ToolRegistry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ToolRegistry")
            .field("count", &self.count())
            .finish()
    }
}

fn normalized_description(description: &str) -> Option<&str> {
    let trimmed = description.trim();
    (!trimmed.is_empty()).then_some(trimmed)
}

#[cfg(test)]
mod tests {
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
