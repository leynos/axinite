//! WASM tool preparation before registry insertion.

use std::sync::Arc;

use crate::secrets::{CredentialMapping, SecretsStore};
use crate::tools::wasm::{Capabilities, OAuthRefreshConfig, WasmError, WasmToolWrapper};

use super::loader::WasmToolRegistration;

pub(super) struct PreparedWasmTool {
    pub(super) wrapper: WasmToolWrapper,
    pub(super) credential_mappings: Vec<CredentialMapping>,
}

pub(super) async fn prepare_wasm_tool(
    reg: WasmToolRegistration<'_>,
) -> Result<PreparedWasmTool, WasmError> {
    let prepared = reg
        .runtime
        .prepare(reg.name, reg.wasm_bytes, reg.limits)
        .await?;

    let credential_mappings = credential_mappings_from_capabilities(&reg.capabilities);
    let hints = WasmMetadataHints {
        name: reg.name,
        description: reg.description,
        schema: reg.schema,
    };
    let runtime_config = WasmRuntimeConfig {
        secrets_store: reg.secrets_store,
        oauth_refresh: reg.oauth_refresh,
    };

    let wrapper = WasmToolWrapper::new(Arc::clone(reg.runtime), prepared, reg.capabilities);
    let wrapper = recover_guest_metadata(wrapper, &hints);
    let wrapper = apply_wasm_overrides(wrapper, hints, runtime_config);

    Ok(PreparedWasmTool {
        wrapper,
        credential_mappings,
    })
}

fn credential_mappings_from_capabilities(capabilities: &Capabilities) -> Vec<CredentialMapping> {
    capabilities
        .http
        .as_ref()
        .map(|http| http.credentials.values().cloned().collect())
        .unwrap_or_default()
}

/// Descriptive metadata hints for WASM tool registration.
struct WasmMetadataHints<'a> {
    name: &'a str,
    description: Option<&'a str>,
    schema: Option<serde_json::Value>,
}

/// Runtime configuration for WASM tool registration.
struct WasmRuntimeConfig {
    secrets_store: Option<Arc<dyn SecretsStore + Send + Sync>>,
    oauth_refresh: Option<OAuthRefreshConfig>,
}

fn recover_guest_metadata(
    mut wrapper: WasmToolWrapper,
    hints: &WasmMetadataHints<'_>,
) -> WasmToolWrapper {
    if hints.description.is_some() && hints.schema.is_some() {
        return wrapper;
    }
    match wrapper.exported_metadata() {
        Ok((description, schema)) => {
            if hints.description.is_none() {
                wrapper = wrapper.with_description(description);
            }
            if hints.schema.is_none() {
                wrapper = wrapper.with_schema(schema);
            }
        }
        Err(error) => {
            if hints.schema.is_none() {
                tracing::warn!(
                    name = hints.name,
                    %error,
                    "Failed to recover exported WASM metadata; tool will be advertised \
                     with placeholder schema until a valid override is provided"
                );
            } else {
                tracing::debug!(
                    name = hints.name,
                    %error,
                    "Failed to recover exported WASM description; using placeholder or override"
                );
            }
        }
    }
    wrapper
}

fn apply_wasm_overrides(
    mut wrapper: WasmToolWrapper,
    hints: WasmMetadataHints<'_>,
    runtime_config: WasmRuntimeConfig,
) -> WasmToolWrapper {
    if let Some(desc) = hints.description {
        wrapper = wrapper.with_description(desc);
    }
    if let Some(s) = hints.schema {
        wrapper = wrapper.with_schema(s);
    }
    if let Some(store) = runtime_config.secrets_store {
        wrapper = wrapper.with_secrets_store(store);
    }
    if let Some(oauth) = runtime_config.oauth_refresh {
        wrapper = wrapper.with_oauth_refresh(oauth);
    }
    wrapper
}
