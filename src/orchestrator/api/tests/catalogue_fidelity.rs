//! Tests for catalogue schema fidelity and versioning determinism.

use std::sync::Arc;

use rstest::rstest;

use super::super::remote_tools::hosted_remote_tool_catalog;

/// Upper bound for reading hosted remote-tool catalogue response bodies in
/// tests. Large enough to accommodate growth in the canonical fixture without
/// triggering a silent truncation.
const MAX_REMOTE_TOOL_CATALOG_BYTES: usize = 64 * 1024;
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

    let body = axum::body::to_bytes(resp.into_body(), MAX_REMOTE_TOOL_CATALOG_BYTES)
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
        .register(build_tool_fixture(
            ToolFixture::CatalogAlphaWithDifferentPayload,
        ))
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
        "different catalogue payloads must produce different catalog versions"
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
