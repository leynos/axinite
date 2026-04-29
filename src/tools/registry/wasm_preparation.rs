//! WASM tool preparation before registry insertion.
//!
//! This module owns the transformation from [`WasmToolRegistration`] to a
//! runtime-ready [`WasmToolWrapper`]. It sits between
//! [`loader::WasmToolRegistration`](super::loader::WasmToolRegistration), which
//! carries caller-supplied registration inputs, and
//! [`ToolRegistry`](super::loader::ToolRegistry) insertion, which publishes the
//! prepared wrapper only after this module has validated and normalized the
//! registration data.
//!
//! Inputs include the raw WASM bytes, runtime, [`Capabilities`], optional
//! description and schema overrides, optional [`SecretsStore`] access, and
//! optional [`OAuthRefreshConfig`]. The main output is a [`PreparedWasmTool`]
//! containing the [`WasmToolWrapper`] plus any [`CredentialMapping`] values that
//! the registry must persist after successful insertion.
//!
//! Preparation compiles and validates the component through the runtime,
//! recovers guest-exported metadata when overrides are absent, applies explicit
//! overrides, and attaches runtime concerns such as secret resolution and OAuth
//! refresh configuration. It returns [`WasmError`] for compile, validation, and
//! configuration failures surfaced by the runtime.
//!
//! The module does not insert tools into the registry or persist credential
//! mappings. It guarantees that credential mappings are kept separate until
//! insertion succeeds, that explicit metadata overrides take precedence over
//! guest exports, and that secret material is not read during preparation; only
//! the store handle is attached for later execution-time lookup.

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

pub(super) fn credential_mappings_from_capabilities(
    capabilities: &Capabilities,
) -> Vec<CredentialMapping> {
    capabilities
        .http
        .as_ref()
        .map(|http| http.credentials.values().cloned().collect())
        .unwrap_or_default()
}

/// Descriptive metadata hints for WASM tool registration.
pub(super) struct WasmMetadataHints<'a> {
    pub(super) name: &'a str,
    pub(super) description: Option<&'a str>,
    pub(super) schema: Option<serde_json::Value>,
}

/// Runtime configuration for WASM tool registration.
pub(super) struct WasmRuntimeConfig {
    pub(super) secrets_store: Option<Arc<dyn SecretsStore + Send + Sync>>,
    pub(super) oauth_refresh: Option<OAuthRefreshConfig>,
}

pub(super) fn recover_guest_metadata(
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

pub(super) fn apply_wasm_overrides(
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
