//! WASM registration types and helpers for the tool registry.

use std::sync::Arc;

use crate::secrets::SecretsStore;
use crate::tools::wasm::{
    Capabilities, OAuthRefreshConfig, PreparedModule, ResourceLimits, WasmError, WasmStorageError,
    WasmToolRuntime, WasmToolStore, WasmToolWrapper,
};

use super::ToolRegistry;

type CompiledWasm = Arc<PreparedModule>;

/// Error when registering a WASM tool from storage.
#[derive(Debug, thiserror::Error)]
pub enum WasmRegistrationError {
    /// Storage-related registration failure from [`WasmStorageError`].
    #[error("Storage error: {0}")]
    Storage(#[from] WasmStorageError),

    /// WASM compilation or wrapper-registration failure from [`WasmError`].
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

/// Arguments for registering a WASM tool loaded from persistent storage.
#[derive(Clone, Copy)]
pub struct WasmFromStorageArgs<'a> {
    /// Backing store used to load the tool record and binary.
    pub store: &'a dyn WasmToolStore,
    /// WASM runtime used to prepare the stored component.
    pub runtime: &'a Arc<WasmToolRuntime>,
    /// User whose installed tool should be loaded.
    pub user_id: &'a str,
    /// Name of the stored tool to load.
    pub name: &'a str,
}

impl ToolRegistry {
    async fn compile_wasm_artifact(
        reg: &WasmToolRegistration<'_>,
    ) -> Result<CompiledWasm, WasmError> {
        reg.runtime
            .prepare(reg.name, reg.wasm_bytes, reg.limits.clone())
            .await
    }

    fn build_wasm_wrapper(
        runtime: &Arc<WasmToolRuntime>,
        compiled: &CompiledWasm,
        capabilities: &Capabilities,
    ) -> Result<WasmToolWrapper, WasmError> {
        Ok(WasmToolWrapper::new(
            Arc::clone(runtime),
            Arc::clone(compiled),
            capabilities.clone(),
        ))
    }

    fn resolve_metadata_overrides(
        mut wrapper: WasmToolWrapper,
        name: &str,
        description: Option<&str>,
        schema: Option<serde_json::Value>,
    ) -> Result<WasmToolWrapper, WasmError> {
        let mut schema = schema;

        if description.is_none() || schema.is_none() {
            match wrapper.exported_metadata() {
                Ok((exported_description, exported_schema)) => {
                    if description.is_none() {
                        wrapper = wrapper.with_description(exported_description);
                    }
                    if schema.is_none() {
                        schema = Some(exported_schema);
                    }
                }
                Err(error) => {
                    tracing::debug!(
                        name = name,
                        %error,
                        "Failed to recover exported WASM metadata; using placeholders or overrides"
                    );
                }
            }
        }

        if let Some(description) = description {
            wrapper = wrapper.with_description(description);
        }
        if let Some(schema) = schema {
            wrapper = wrapper.with_schema(schema);
        }

        Ok(wrapper)
    }

    fn apply_security_integrations(
        mut wrapper: WasmToolWrapper,
        secrets_store: Option<&Arc<dyn SecretsStore + Send + Sync>>,
        oauth_refresh: Option<&OAuthRefreshConfig>,
    ) -> WasmToolWrapper {
        if let Some(store) = secrets_store {
            wrapper = wrapper.with_secrets_store(Arc::clone(store));
        }
        if let Some(oauth) = oauth_refresh {
            wrapper = wrapper.with_oauth_refresh(oauth.clone());
        }
        wrapper
    }

    async fn register_wrapper_and_credentials(
        &self,
        name: &str,
        wrapper: WasmToolWrapper,
        capabilities: &Capabilities,
    ) -> Result<(), WasmError> {
        let credential_mappings: Vec<crate::secrets::CredentialMapping> = capabilities
            .http
            .as_ref()
            .map(|http| http.credentials.values().cloned().collect())
            .unwrap_or_default();

        self.register(Arc::new(wrapper)).await;
        self.register_wasm_credential_mappings(name, credential_mappings);

        tracing::debug!(name = name, "Registered WASM tool");
        Ok(())
    }

    /// Register a WASM tool from bytes.
    ///
    /// This validates and compiles the WASM component, then registers it as a tool.
    /// The tool will be executed in a sandboxed environment with the given capabilities.
    pub async fn register_wasm(&self, reg: WasmToolRegistration<'_>) -> Result<(), WasmError> {
        let compiled = Self::compile_wasm_artifact(&reg).await?;
        let wrapper = Self::build_wasm_wrapper(reg.runtime, &compiled, &reg.capabilities)?;
        let wrapper =
            Self::resolve_metadata_overrides(wrapper, reg.name, reg.description, reg.schema)?;
        let wrapper = Self::apply_security_integrations(
            wrapper,
            reg.secrets_store.as_ref(),
            reg.oauth_refresh.as_ref(),
        );

        self.register_wrapper_and_credentials(reg.name, wrapper, &reg.capabilities)
            .await
    }

    /// Register a WASM tool from database storage.
    ///
    /// Loads the WASM binary with integrity verification and configures capabilities.
    pub async fn register_wasm_from_storage(
        &self,
        args: WasmFromStorageArgs<'_>,
    ) -> Result<(), WasmRegistrationError> {
        let WasmFromStorageArgs {
            store,
            runtime,
            user_id,
            name,
        } = args;
        let tool_with_binary = store
            .get_with_binary(user_id, name)
            .await
            .map_err(WasmRegistrationError::Storage)?;

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
            trust_level = %tool_with_binary.tool.trust_level,
            "Registered WASM tool from storage"
        );

        Ok(())
    }

    fn register_wasm_credential_mappings(
        &self,
        tool_name: &str,
        credential_mappings: Vec<crate::secrets::CredentialMapping>,
    ) {
        if let Some(registry) = &self.credential_registry
            && !credential_mappings.is_empty()
        {
            let count = credential_mappings.len();
            registry.add_mappings(credential_mappings);
            tracing::debug!(
                name = tool_name,
                credential_count = count,
                "Added credential mappings from WASM tool"
            );
        }
    }
}

fn normalized_description(description: &str) -> Option<&str> {
    let trimmed = description.trim();
    (!trimmed.is_empty()).then_some(trimmed)
}

fn normalized_schema(schema: serde_json::Value) -> Option<serde_json::Value> {
    match schema {
        serde_json::Value::Null => None,
        serde_json::Value::String(value) => {
            let trimmed = value.trim();
            if trimmed.is_empty() || trimmed.eq_ignore_ascii_case("null") {
                None
            } else {
                Some(serde_json::Value::String(trimmed.to_string()))
            }
        }
        value => Some(value),
    }
}
