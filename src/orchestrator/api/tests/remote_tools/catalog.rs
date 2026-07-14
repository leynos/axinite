//! Tests for the hosted remote-tool catalogue endpoint and its visibility
//! filtering, ordering, and versioning.

use std::sync::Arc;

use rstest::rstest;

use super::super::super::remote_tools::hosted_remote_tool_catalog;
use super::super::fixtures::remote_tool_mocks::{
    StubOutput, StubTool, ToolFixture, build_tool_fixture,
};
use super::super::fixtures::test_state;
use super::super::*;
use crate::tools::HostedToolCatalogSource;
use crate::worker::api::REMOTE_TOOL_CATALOG_ROUTE;

/// Register the full set of catalogue-visibility test fixtures into `tools`.
///
/// Registers one hosted-safe tool, two protected tools (`create_job`,
/// `job_events`), one approval-gated tool, and one container-only tool,
/// covering every filtering branch exercised by the catalogue visibility tests.
async fn populate_catalog_visibility_fixtures(tools: &ToolRegistry) {
    tools
        .register(build_tool_fixture(ToolFixture::CatalogAlpha))
        .await;
    tools
        .register(build_tool_fixture(ToolFixture::CatalogWasm))
        .await;
    tools
        .register(Arc::new(StubTool {
            name: "hosted_extension_catalog_builtin",
            description: "Hosted-safe extension-management built-in".to_string(),
            catalog_source: None,
            output: StubOutput::Fixed(serde_json::json!({"extensions": []})),
            ..StubTool::hosted(
                "hosted_extension_catalog_builtin",
                "",
                serde_json::json!({"type": "object", "properties": {}}),
            )
        }))
        .await;
    tools
        .register(Arc::new(StubTool {
            name: "create_job",
            description: "Protected orchestration tool".to_string(),
            output: StubOutput::Fixed(serde_json::json!({"created": true})),
            ..StubTool::hosted(
                "create_job",
                "",
                serde_json::json!({"type": "object", "properties": {}}),
            )
        }))
        .await;
    tools
        .register(Arc::new(StubTool {
            name: "job_events",
            description: "Protected job-events tool".to_string(),
            output: StubOutput::Fixed(serde_json::json!({"events": []})),
            ..StubTool::hosted(
                "job_events",
                "",
                serde_json::json!({"type": "object", "properties": {}}),
            )
        }))
        .await;
    tools
        .register(build_tool_fixture(ToolFixture::ApprovalGated))
        .await;
    tools
        .register(build_tool_fixture(ToolFixture::ContainerOnly))
        .await;
}

fn make_catalog_fixture_definition(
    name: &str,
    description: &str,
    param_name: &str,
    param_description: &str,
) -> crate::llm::ToolDefinition {
    crate::llm::ToolDefinition {
        name: name.to_string(),
        description: description.to_string(),
        parameters: serde_json::json!({
            "type": "object",
            "properties": {param_name: {"type": "string", "description": param_description}},
            "required": [param_name]
        }),
    }
}

fn expected_catalog_fixture_definition() -> crate::llm::ToolDefinition {
    make_catalog_fixture_definition(
        "remote_tool_catalog_fixture",
        "Hosted-safe tool for catalog tests",
        "query",
        "search query",
    )
}

fn expected_catalog_wasm_fixture_definition() -> crate::llm::ToolDefinition {
    make_catalog_fixture_definition(
        "remote_tool_catalog_fixture_wasm",
        "Hosted-safe WASM tool for catalog tests",
        "repository",
        "repository name",
    )
}

#[rstest]
#[tokio::test]
async fn remote_tool_catalog_returns_hosted_safe_tool_definitions(test_state: OrchestratorState) {
    populate_catalog_visibility_fixtures(&test_state.tools).await;
    let job_id = Uuid::new_v4();
    let token = test_state.token_store.create_token(job_id).await;
    let router = OrchestratorApi::router(test_state);

    let req = Request::builder()
        .method("GET")
        .uri(REMOTE_TOOL_CATALOG_ROUTE.replace("{job_id}", &job_id.to_string()))
        .header("Authorization", format!("Bearer {}", token))
        .body(Body::empty())
        .expect("build hosted remote-tool catalog request");

    let resp = router
        .oneshot(req)
        .await
        .expect("send hosted remote-tool catalog request");
    assert_eq!(resp.status(), StatusCode::OK);

    let body = axum::body::to_bytes(resp.into_body(), 4096)
        .await
        .expect("read hosted remote-tool catalog response body");
    let catalog: crate::worker::api::RemoteToolCatalogResponse =
        serde_json::from_slice(&body).expect("parse hosted remote-tool catalog response");

    assert_eq!(catalog.toolset_instructions, Vec::<String>::new());
    assert_ne!(catalog.catalog_version, 0);
    assert_eq!(catalog.tools.len(), 2);
    let expected = vec![
        expected_catalog_fixture_definition(),
        expected_catalog_wasm_fixture_definition(),
    ];
    assert_eq!(catalog.tools, expected);
}

async fn assert_catalog_excludes_stub(excluded_stub: StubTool) {
    let registry = Arc::new(ToolRegistry::new());
    registry
        .register(build_tool_fixture(ToolFixture::CatalogAlpha))
        .await;
    registry.register(Arc::new(excluded_stub)).await;

    let (tools, _instructions, _version) = hosted_remote_tool_catalog(&registry).await;

    assert_eq!(
        tools
            .iter()
            .map(|tool| tool.name.as_str())
            .collect::<Vec<_>>(),
        vec!["remote_tool_catalog_fixture"]
    );
}

#[rstest]
#[case(
    "job_events",
    "Protected job-events tool",
    Some(HostedToolCatalogSource::Mcp),
    serde_json::json!({"events": []})
)]
#[case(
    "hosted_extension_catalog_builtin",
    "Hosted-safe extension-management built-in",
    None,
    serde_json::json!({"extensions": []})
)]
#[tokio::test]
async fn remote_tool_catalog_excludes_ineligible_tools(
    #[case] name: &'static str,
    #[case] description: &'static str,
    #[case] catalog_source: Option<HostedToolCatalogSource>,
    #[case] output: serde_json::Value,
) {
    assert_catalog_excludes_stub(StubTool {
        name,
        description: description.to_string(),
        catalog_source,
        output: StubOutput::Fixed(output),
        ..StubTool::hosted(
            name,
            "",
            serde_json::json!({"type": "object", "properties": {}}),
        )
    })
    .await;
}

#[tokio::test]
async fn remote_tool_catalog_sorts_tools_before_versioning() {
    let registry_a = Arc::new(ToolRegistry::new());
    registry_a
        .register(build_tool_fixture(ToolFixture::CatalogBeta))
        .await;
    registry_a
        .register(build_tool_fixture(ToolFixture::CatalogAlpha))
        .await;

    let registry_b = Arc::new(ToolRegistry::new());
    registry_b
        .register(build_tool_fixture(ToolFixture::CatalogAlpha))
        .await;
    registry_b
        .register(build_tool_fixture(ToolFixture::CatalogBeta))
        .await;

    let (tools_a, instructions_a, version_a) = hosted_remote_tool_catalog(&registry_a).await;
    let (tools_b, instructions_b, version_b) = hosted_remote_tool_catalog(&registry_b).await;

    assert_eq!(
        tools_a.iter().map(|t| t.name.as_str()).collect::<Vec<_>>(),
        vec![
            "remote_tool_catalog_fixture",
            "remote_tool_catalog_fixture_beta"
        ]
    );
    assert_eq!(
        tools_b.iter().map(|t| t.name.as_str()).collect::<Vec<_>>(),
        vec![
            "remote_tool_catalog_fixture",
            "remote_tool_catalog_fixture_beta"
        ]
    );
    assert_eq!(instructions_a, instructions_b);
    assert_eq!(version_a, version_b);
}
