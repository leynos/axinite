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

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use rstest::rstest;

    use super::{
        PreparedWasmTool, WasmMetadataHints, WasmRuntimeConfig, apply_wasm_overrides,
        credential_mappings_from_capabilities, prepare_wasm_tool, recover_guest_metadata,
    };
    use crate::secrets::CredentialMapping;
    use crate::testing::{github_wasm_artifact, metadata_test_runtime};
    use crate::tools::tool::NativeTool;
    use crate::tools::wasm::{Capabilities, HttpCapability, OAuthRefreshConfig, WasmToolWrapper};

    use super::WasmToolRegistration;

    #[test]
    fn credential_mappings_from_capabilities_returns_empty_vec_without_http() {
        assert!(credential_mappings_from_capabilities(&Capabilities::default()).is_empty());
    }

    #[test]
    fn credential_mappings_from_capabilities_collects_http_credentials() {
        let capabilities = Capabilities::default().with_http(
            HttpCapability::default()
                .with_credential("api", CredentialMapping::bearer("api", "api.example.com"))
                .with_credential(
                    "admin",
                    CredentialMapping::header("admin", "X-Admin-Token", "admin.example.com"),
                ),
        );

        let mut mappings = credential_mappings_from_capabilities(&capabilities);
        mappings.sort_by(|left, right| left.secret_name.cmp(&right.secret_name));

        assert_eq!(mappings.len(), 2);
        assert_eq!(mappings[0].secret_name, "admin");
        assert_eq!(mappings[0].host_patterns, ["admin.example.com"]);
        assert_eq!(mappings[1].secret_name, "api");
        assert_eq!(mappings[1].host_patterns, ["api.example.com"]);
    }

    #[tokio::test]
    async fn prepare_wasm_tool_prepares_wrapper_and_collects_credentials() {
        let runtime = metadata_test_runtime().expect("create metadata test runtime");
        let wasm_path = github_wasm_artifact().expect("build or find github WASM artifact");
        let wasm_bytes = std::fs::read(wasm_path).expect("read github wasm artifact");
        let capabilities =
            Capabilities::default().with_http(HttpCapability::default().with_credential(
                "github",
                CredentialMapping::bearer("github_token", "api.github.com"),
            ));
        let expected_credential_mappings = credential_mappings_from_capabilities(&capabilities);
        let schema_override = serde_json::json!({
            "type": "object",
            "properties": {
                "query": {"type": "string"}
            },
            "required": ["query"]
        });

        let prepared: PreparedWasmTool = prepare_wasm_tool(WasmToolRegistration {
            name: "github_prepare",
            wasm_bytes: &wasm_bytes,
            runtime: &runtime,
            capabilities,
            limits: None,
            description: None,
            schema: Some(schema_override.clone()),
            secrets_store: None,
            oauth_refresh: None,
        })
        .await
        .expect("prepare WASM tool");

        assert_eq!(prepared.wrapper.name(), "github_prepare");
        assert_eq!(
            prepared.wrapper.description(),
            concat!(
                "GitHub integration for managing repositories, issues, pull requests, and ",
                "workflows. Supports reading repo info, listing/creating issues, reviewing ",
                "PRs, and triggering GitHub Actions. Authentication is handled via the ",
                "'github_token' secret injected by the host."
            )
        );
        assert_eq!(prepared.wrapper.parameters_schema(), schema_override);
        assert_eq!(
            prepared.credential_mappings.len(),
            expected_credential_mappings.len()
        );
        assert_eq!(
            prepared.credential_mappings[0].secret_name,
            expected_credential_mappings[0].secret_name
        );
        assert_eq!(
            prepared.credential_mappings[0].host_patterns,
            expected_credential_mappings[0].host_patterns
        );
        assert_eq!(
            format!("{:?}", prepared.credential_mappings[0].location),
            format!("{:?}", expected_credential_mappings[0].location)
        );
    }

    #[tokio::test]
    async fn recover_guest_metadata_returns_early_when_both_overrides_are_present() {
        let wrapper = wasm_wrapper("metadata_overrides")
            .await
            .with_description("placeholder before override")
            .with_schema(serde_json::json!({"type": "object"}));

        let recovered = recover_guest_metadata(
            wrapper,
            &WasmMetadataHints {
                name: "metadata_overrides",
                description: Some("description override"),
                schema: Some(serde_json::json!({"type": "string"})),
            },
        );

        assert_eq!(recovered.description(), "placeholder before override");
        assert_eq!(
            recovered.parameters_schema(),
            serde_json::json!({"type": "object"})
        );
    }

    #[tokio::test]
    async fn recover_guest_metadata_populates_missing_description_from_guest() {
        let wrapper = wasm_wrapper("metadata_description").await;
        let schema_override = serde_json::json!({
            "type": "object",
            "properties": {
                "issue": {"type": "string"}
            }
        });

        let recovered = recover_guest_metadata(
            wrapper,
            &WasmMetadataHints {
                name: "metadata_description",
                description: None,
                schema: Some(schema_override.clone()),
            },
        );

        assert_eq!(
            recovered.description(),
            concat!(
                "GitHub integration for managing repositories, issues, pull requests, and ",
                "workflows. Supports reading repo info, listing/creating issues, reviewing ",
                "PRs, and triggering GitHub Actions. Authentication is handled via the ",
                "'github_token' secret injected by the host."
            )
        );
        assert_eq!(
            recovered.parameters_schema(),
            serde_json::json!({
                "type": "object",
                "properties": {},
                "additionalProperties": true
            })
        );
    }

    #[tokio::test]
    async fn recover_guest_metadata_keeps_placeholders_when_export_fails() {
        let runtime = metadata_test_runtime().expect("create metadata test runtime");
        let prepared = runtime
            .prepare("broken_metadata", b"\0asm\r\0\x01\0", None)
            .await
            .expect("prepare core wasm module as an invalid tool component");
        let wrapper = WasmToolWrapper::new(runtime, prepared, Capabilities::default());

        let recovered = recover_guest_metadata(
            wrapper,
            &WasmMetadataHints {
                name: "broken_metadata",
                description: None,
                schema: None,
            },
        );

        assert_eq!(recovered.description(), "WASM sandboxed tool");
        assert_eq!(
            recovered.parameters_schema(),
            serde_json::json!({
                "type": "object",
                "properties": {},
                "additionalProperties": true
            })
        );
    }

    #[rstest]
    #[case::none(None, None, false, false)]
    #[case::description(Some("override description"), None, false, false)]
    #[case::schema(None, Some(serde_json::json!({"type": "string"})), false, false)]
    #[case::secrets(None, None, true, false)]
    #[case::oauth(None, None, false, true)]
    #[tokio::test]
    async fn apply_wasm_overrides_applies_each_optional_field(
        #[case] description: Option<&str>,
        #[case] schema: Option<serde_json::Value>,
        #[case] include_secrets_store: bool,
        #[case] include_oauth_refresh: bool,
    ) {
        let wrapper = wasm_wrapper("apply_overrides")
            .await
            .with_description("original description")
            .with_schema(serde_json::json!({"type": "object"}));
        let secrets_store = include_secrets_store
            .then(|| Arc::new(crate::testing::credentials::test_secrets_store()) as Arc<_>);
        let oauth_refresh = include_oauth_refresh.then(|| OAuthRefreshConfig {
            token_url: "https://auth.example.com/token".to_string(),
            client_id: "client-id".to_string(),
            client_secret: Some("client-secret".to_string()),
            secret_name: "oauth_token".to_string(),
            provider: Some("example".to_string()),
        });

        let wrapper = apply_wasm_overrides(
            wrapper,
            WasmMetadataHints {
                name: "apply_overrides",
                description,
                schema: schema.clone(),
            },
            WasmRuntimeConfig {
                secrets_store,
                oauth_refresh,
            },
        );

        assert_eq!(
            wrapper.description(),
            description.unwrap_or("original description")
        );
        assert_eq!(
            wrapper.parameters_schema(),
            schema.unwrap_or_else(|| serde_json::json!({"type": "object"}))
        );
    }

    async fn wasm_wrapper(name: &str) -> WasmToolWrapper {
        let runtime = metadata_test_runtime().expect("create metadata test runtime");
        let wasm_path = github_wasm_artifact().expect("build or find github WASM artifact");
        let wasm_bytes = std::fs::read(wasm_path).expect("read github wasm artifact");
        let prepared = runtime
            .prepare(name, &wasm_bytes, None)
            .await
            .expect("prepare github WASM fixture");

        WasmToolWrapper::new(runtime, prepared, Capabilities::default())
    }
}
