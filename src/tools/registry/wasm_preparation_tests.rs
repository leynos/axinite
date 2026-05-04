//! Unit and async integration tests for the WASM tool preparation pipeline.
//!
//! Exercises the helpers in [`super::wasm_preparation`]:
//!
//! - [`credential_mappings_from_capabilities`] — empty and populated capability sets.
//! - [`prepare_wasm_tool`] — end-to-end preparation, wrapper identity, and credential
//!   mapping extraction.
//! - [`recover_guest_metadata`] — early-return when both overrides are present,
//!   population from guest exports when description is absent, and placeholder
//!   retention when guest export fails.
//! - [`apply_wasm_overrides`] — conditional application of description, schema,
//!   secrets store, and OAuth refresh configuration.

use std::sync::Arc;

use anyhow::Result;
use rstest::rstest;

use super::WasmToolRegistration;
use super::wasm_preparation::{
    PreparedWasmTool, WasmMetadataHints, WasmRuntimeConfig, apply_wasm_overrides,
    credential_mappings_from_capabilities, prepare_wasm_tool, recover_guest_metadata,
};
use crate::secrets::CredentialMapping;
use crate::testing::{github_wasm_artifact, metadata_test_runtime};
use crate::tools::tool::NativeTool;
use crate::tools::wasm::{Capabilities, HttpCapability, OAuthRefreshConfig, WasmToolWrapper};

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
async fn prepare_wasm_tool_prepares_wrapper_and_collects_credentials() -> Result<()> {
    let runtime = metadata_test_runtime()?;
    let wasm_path = github_wasm_artifact()?;
    let wasm_bytes = std::fs::read(wasm_path)?;
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
    .await?;

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

    Ok(())
}

#[tokio::test]
async fn recover_guest_metadata_returns_early_when_both_overrides_are_present() -> Result<()> {
    let wrapper = wasm_wrapper("metadata_overrides")
        .await?
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

    Ok(())
}

#[tokio::test]
async fn recover_guest_metadata_populates_missing_description_from_guest() -> Result<()> {
    let wrapper = wasm_wrapper("metadata_description").await?;
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

    Ok(())
}

#[tokio::test]
async fn recover_guest_metadata_keeps_placeholders_when_export_fails() -> Result<()> {
    let runtime = metadata_test_runtime()?;
    let prepared = runtime
        .prepare("broken_metadata", b"\0asm\r\0\x01\0", None)
        .await?;
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

    Ok(())
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
) -> Result<()> {
    let wrapper = wasm_wrapper("apply_overrides")
        .await?
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
    assert_eq!(wrapper.secrets_store().is_some(), include_secrets_store);
    assert_eq!(wrapper.oauth_refresh().is_some(), include_oauth_refresh);
    if include_oauth_refresh {
        assert_eq!(
            wrapper
                .oauth_refresh()
                .map(|oauth| oauth.secret_name.as_str()),
            Some("oauth_token")
        );
    }

    Ok(())
}

async fn wasm_wrapper(name: &str) -> Result<WasmToolWrapper> {
    let runtime = metadata_test_runtime()?;
    let wasm_path = github_wasm_artifact()?;
    let wasm_bytes = std::fs::read(wasm_path)?;
    let prepared = runtime.prepare(name, &wasm_bytes, None).await?;

    Ok(WasmToolWrapper::new(
        runtime,
        prepared,
        Capabilities::default(),
    ))
}
