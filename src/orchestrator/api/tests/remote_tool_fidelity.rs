//! Tests for schema fidelity, catalogue versioning determinism, and
//! serialisation round-trips of shared orchestrator–worker transport types.

use std::sync::Arc;

use rstest::rstest;

use super::super::remote_tools::hosted_remote_tool_catalog;
use super::fixtures::remote_tool_mocks::{
    ToolFixture, build_tool_fixture, complex_tool_definition, complex_tool_stub,
};
use super::fixtures::test_state;
use super::*;
use crate::tools::ToolRegistry;
use crate::worker::api::REMOTE_TOOL_CATALOG_ROUTE;

#[rstest]
#[tokio::test]
async fn remote_tool_catalog_preserves_full_tool_definition_payload(test_state: OrchestratorState) {
    let expected = complex_tool_definition();
    test_state
        .tools
        .register(Arc::new(complex_tool_stub()))
        .await;

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

    let tool = catalog
        .tools
        .iter()
        .find(|t| t.name == expected.name)
        .expect("complex tool should be in catalogue");

    assert_eq!(
        tool, &expected,
        "catalogue tool definition must match the registry definition exactly"
    );
}

#[tokio::test]
async fn remote_tool_catalog_version_is_deterministic_and_sensitive_to_content() {
    let registry_a = Arc::new(ToolRegistry::new());
    registry_a
        .register(build_tool_fixture(ToolFixture::CatalogAlpha))
        .await;

    let registry_b = Arc::new(ToolRegistry::new());
    registry_b
        .register(build_tool_fixture(ToolFixture::CatalogAlpha))
        .await;

    let registry_c = Arc::new(ToolRegistry::new());
    registry_c
        .register(build_tool_fixture(ToolFixture::CatalogBeta))
        .await;

    let (_tools_a, _instructions_a, version_a) = hosted_remote_tool_catalog(&registry_a).await;
    let (_tools_b, _instructions_b, version_b) = hosted_remote_tool_catalog(&registry_b).await;
    let (_tools_c, _instructions_c, version_c) = hosted_remote_tool_catalog(&registry_c).await;

    assert_eq!(
        version_a, version_b,
        "identical tool sets must produce identical catalog versions"
    );
    assert_ne!(
        version_a, version_c,
        "different tool sets must produce different catalog versions"
    );
}

#[tokio::test]
async fn remote_tool_catalog_version_independent_of_registration_order() {
    let registry_a = Arc::new(ToolRegistry::new());
    registry_a
        .register(build_tool_fixture(ToolFixture::CatalogAlpha))
        .await;
    registry_a
        .register(build_tool_fixture(ToolFixture::CatalogBeta))
        .await;

    let registry_b = Arc::new(ToolRegistry::new());
    registry_b
        .register(build_tool_fixture(ToolFixture::CatalogBeta))
        .await;
    registry_b
        .register(build_tool_fixture(ToolFixture::CatalogAlpha))
        .await;

    let (_tools_a, _instructions_a, version_a) = hosted_remote_tool_catalog(&registry_a).await;
    let (_tools_b, _instructions_b, version_b) = hosted_remote_tool_catalog(&registry_b).await;

    assert_eq!(
        version_a, version_b,
        "catalog version must be independent of tool registration order"
    );
}

#[tokio::test]
async fn orchestrator_catalog_response_round_trips_through_worker_shared_types() {
    let registry = Arc::new(ToolRegistry::new());
    registry.register(Arc::new(complex_tool_stub())).await;

    let (tools, instructions, version) = hosted_remote_tool_catalog(&registry).await;

    let catalog_response = crate::worker::api::RemoteToolCatalogResponse {
        tools: tools.clone(),
        toolset_instructions: instructions.clone(),
        catalog_version: version,
    };

    let serialized = serde_json::to_string(&catalog_response)
        .expect("serialize orchestrator-built catalog response");
    let deserialized: crate::worker::api::RemoteToolCatalogResponse =
        serde_json::from_str(&serialized)
            .expect("orchestrator response must deserialize into shared type");

    assert_eq!(
        deserialized, catalog_response,
        "catalog response must round-trip without field loss"
    );
}

#[tokio::test]
async fn worker_execution_request_round_trips_through_shared_types() {
    let execution_request = crate::worker::api::RemoteToolExecutionRequest {
        tool_name: "remote_tool_fidelity_fixture".to_string(),
        params: serde_json::json!({"query": "test", "options": {"limit": 10}}),
    };

    let serialized = serde_json::to_string(&execution_request)
        .expect("serialize worker-built execution request");
    let deserialized: crate::worker::api::RemoteToolExecutionRequest =
        serde_json::from_str(&serialized)
            .expect("worker request must deserialize into shared type");

    assert_eq!(deserialized.tool_name, execution_request.tool_name);
    assert_eq!(deserialized.params, execution_request.params);
}

#[tokio::test]
async fn orchestrator_execution_response_round_trips_through_worker_shared_types() {
    let execution_output = crate::tools::ToolOutput::success(
        serde_json::json!({"result": "executed"}),
        std::time::Duration::from_millis(15),
    )
    .with_cost(rust_decimal::Decimal::new(200, 2))
    .with_raw("orchestrator tool output");

    let execution_response = crate::worker::api::RemoteToolExecutionResponse {
        output: execution_output.clone(),
    };

    let serialized = serde_json::to_string(&execution_response)
        .expect("serialize orchestrator-built execution response");
    let deserialized: crate::worker::api::RemoteToolExecutionResponse =
        serde_json::from_str(&serialized)
            .expect("orchestrator response must deserialize into shared type");

    assert_eq!(deserialized.output.result, execution_output.result);
    assert_eq!(deserialized.output.cost, execution_output.cost);
    assert_eq!(deserialized.output.raw, execution_output.raw);
    assert_eq!(deserialized.output.duration, execution_output.duration);
}
