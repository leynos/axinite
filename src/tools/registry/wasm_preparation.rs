//! WASM tool preparation before registry insertion.
//!
//! This module owns the transformation from [`WasmToolRegistration`] to a
//! runtime-ready [`WasmToolWrapper`]. It sits between
//! [`wasm_registration::WasmToolRegistration`](super::wasm_registration::WasmToolRegistration), which
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

use super::wasm_registration::WasmToolRegistration;

/// Output of the WASM preparation pipeline, ready for registry insertion.
///
/// Carries the compiled and configured [`WasmToolWrapper`] together with any
/// [`CredentialMapping`] values that the registry must persist only after the
/// wrapper has been successfully inserted.
pub(super) struct PreparedWasmTool {
    /// The compiled and configured wrapper ready for registry insertion.
    pub(super) wrapper: WasmToolWrapper,
    /// HTTP credential mappings to be persisted only after the wrapper
    /// has been successfully inserted into the registry.
    pub(super) credential_mappings: Vec<CredentialMapping>,
}

/// Compile, validate, and configure a WASM tool for registry insertion.
///
/// Drives the full preparation pipeline:
///
/// 1. Compiles the component via the runtime.
/// 2. Derives [`CredentialMapping`] values from HTTP capabilities.
/// 3. Builds [`WasmMetadataHints`] and [`WasmRuntimeConfig`] from the registration inputs.
/// 4. Constructs a [`WasmToolWrapper`].
/// 5. Calls [`recover_guest_metadata`] to fill in absent description or schema.
/// 6. Calls [`apply_wasm_overrides`] to enforce explicit overrides and attach runtime
///    concerns.
///
/// Returns a [`PreparedWasmTool`] on success or a [`WasmError`] if compilation,
/// validation, or configuration fails. Does not insert the tool into the registry
/// or persist credential mappings.
pub(super) async fn prepare_wasm_tool(
    reg: WasmToolRegistration<'_>,
) -> Result<PreparedWasmTool, WasmError> {
    let prepared = reg
        .runtime
        .prepare(reg.name, reg.wasm_bytes, reg.limits)
        .await
        .inspect_err(|error| {
            tracing::warn!(
                name = reg.name,
                %error,
                "WASM tool preparation failed"
            );
        })?;

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
    let wrapper = recover_guest_metadata(wrapper, &hints)?;
    let wrapper = apply_wasm_overrides(wrapper, hints, runtime_config);

    Ok(PreparedWasmTool {
        wrapper,
        credential_mappings,
    })
}

/// Extract HTTP credential mappings from a set of [`Capabilities`].
///
/// Returns the values from `capabilities.http.credentials` as a flat `Vec`, or an
/// empty `Vec` when no HTTP capability is present.
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
    /// Unique name under which the tool will be registered.
    pub(super) name: &'a str,
    /// Caller-supplied description override, or `None` to recover from
    /// the guest's exported metadata.
    pub(super) description: Option<&'a str>,
    /// Caller-supplied parameter schema override, or `None` to recover
    /// from the guest's exported metadata.
    pub(super) schema: Option<serde_json::Value>,
}

/// Runtime configuration for WASM tool registration.
pub(super) struct WasmRuntimeConfig {
    /// Secrets store attached to the wrapper for execution-time
    /// credential resolution. Secret material is not read during
    /// preparation.
    pub(super) secrets_store: Option<Arc<dyn SecretsStore + Send + Sync>>,
    /// OAuth refresh configuration for automatic token renewal, or
    /// `None` if the tool does not use OAuth.
    pub(super) oauth_refresh: Option<OAuthRefreshConfig>,
}

/// Populate absent description or schema from the wrapper's guest-exported metadata.
///
/// Returns early without calling `exported_metadata` when both `hints.description`
/// and `hints.schema` are `Some`. On export failure, both branches emit
/// [`tracing::warn`] but use distinct messages depending on whether
/// `hints.schema` is also missing or only the description is missing. Export
/// failures are returned after logging so callers can decide whether to reject
/// registration or recover at a higher boundary.
pub(super) fn recover_guest_metadata(
    wrapper: WasmToolWrapper,
    hints: &WasmMetadataHints<'_>,
) -> Result<WasmToolWrapper, WasmError> {
    if hints.description.is_some() && hints.schema.is_some() {
        return Ok(wrapper);
    }
    match wrapper.exported_metadata() {
        Ok(metadata) => Ok(fill_absent_metadata(wrapper, hints, metadata)),
        Err(error) => {
            warn_metadata_export_failure(hints, &error);
            Err(error)
        }
    }
}

/// Copy guest-exported metadata onto the wrapper for any hint left as `None`.
fn fill_absent_metadata(
    mut wrapper: WasmToolWrapper,
    hints: &WasmMetadataHints<'_>,
    (description, schema): (String, serde_json::Value),
) -> WasmToolWrapper {
    if hints.description.is_none() {
        wrapper = wrapper.with_description(description);
    }
    if hints.schema.is_none() {
        wrapper = wrapper.with_schema(schema);
    }
    wrapper
}

/// Emit the registration-rejection warning appropriate to the missing hints.
fn warn_metadata_export_failure(hints: &WasmMetadataHints<'_>, error: &WasmError) {
    if hints.schema.is_none() {
        tracing::warn!(
            name = hints.name,
            %error,
            "Failed to recover exported WASM metadata; rejecting registration"
        );
    } else {
        tracing::warn!(
            name = hints.name,
            %error,
            "Failed to recover exported WASM description; rejecting registration"
        );
    }
}

/// Apply explicit metadata overrides and attach runtime concerns to a wrapper.
///
/// Each field in `hints` and `runtime_config` is applied only when it is `Some`,
/// leaving the wrapper's existing value intact otherwise.
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
