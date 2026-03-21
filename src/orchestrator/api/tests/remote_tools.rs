//! Tests for hosted remote-tool catalogue fetch and execution.

use std::sync::Arc;

use rstest::rstest;

use super::super::remote_tools::hosted_remote_tool_catalog;
use super::fixtures::remote_tool_helpers::execute_remote_tool_status;
use super::fixtures::remote_tool_mocks::{
    ErrorTool, ExecuteErrorKind, JobAwareTool, StubOutput, StubTool, ToolFixture,
    build_tool_fixture,
};
use super::fixtures::test_state;
use super::*;
use crate::worker::api::{REMOTE_TOOL_CATALOG_ROUTE, REMOTE_TOOL_EXECUTE_ROUTE};

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
        .register(Arc::new(StubTool {
            name: "tool_list",
            description: "Hosted-safe extension-management built-in",
            catalog_source: None,
            output: StubOutput::Fixed(serde_json::json!({"extensions": []})),
            ..StubTool::hosted(
                "tool_list",
                "",
                serde_json::json!({"type": "object", "properties": {}}),
            )
        }))
        .await;
    tools
        .register(Arc::new(StubTool {
            name: "create_job",
            description: "Protected orchestration tool",
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
            description: "Protected job-events tool",
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
    assert_eq!(catalog.tools.len(), 1);
    let tool = &catalog.tools[0];
    assert_eq!(tool.name, "remote_tool_catalog_fixture");
    assert_eq!(tool.description, "Hosted-safe tool for catalog tests");
    assert_eq!(
        tool.parameters,
        serde_json::json!({
            "type": "object",
            "properties": {"query": {"type": "string", "description": "search query"}},
            "required": ["query"]
        })
    );
}

#[tokio::test]
async fn remote_tool_catalog_excludes_job_events_named_tools() {
    let registry = Arc::new(ToolRegistry::new());
    registry
        .register(build_tool_fixture(ToolFixture::CatalogAlpha))
        .await;
    registry
        .register(Arc::new(StubTool {
            name: "job_events",
            description: "Protected job-events tool",
            output: StubOutput::Fixed(serde_json::json!({"events":[]})),
            ..StubTool::hosted(
                "job_events",
                "",
                serde_json::json!({"type":"object","properties":{}}),
            )
        }))
        .await;

    let (tools, _instructions, _version) = hosted_remote_tool_catalog(&registry).await;

    assert_eq!(
        tools
            .iter()
            .map(|tool| tool.name.as_str())
            .collect::<Vec<_>>(),
        vec!["remote_tool_catalog_fixture"]
    );
}

#[tokio::test]
async fn remote_tool_catalog_excludes_non_mcp_orchestrator_tools() {
    let registry = Arc::new(ToolRegistry::new());
    registry
        .register(build_tool_fixture(ToolFixture::CatalogAlpha))
        .await;
    registry
        .register(Arc::new(StubTool {
            name: "tool_list",
            description: "Hosted-safe extension-management built-in",
            catalog_source: None,
            output: StubOutput::Fixed(serde_json::json!({"extensions": []})),
            ..StubTool::hosted(
                "tool_list",
                "",
                serde_json::json!({"type":"object","properties":{}}),
            )
        }))
        .await;

    let (tools, _instructions, _version) = hosted_remote_tool_catalog(&registry).await;

    assert_eq!(
        tools
            .iter()
            .map(|tool| tool.name.as_str())
            .collect::<Vec<_>>(),
        vec!["remote_tool_catalog_fixture"]
    );
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

#[rstest]
#[tokio::test]
async fn remote_tool_execute_rejects_unknown_tools(test_state: OrchestratorState) {
    let job_id = Uuid::new_v4();
    let token = test_state.token_store.create_token(job_id).await;
    let router = OrchestratorApi::router(test_state);

    let req = Request::builder()
        .method("POST")
        .uri(REMOTE_TOOL_EXECUTE_ROUTE.replace("{job_id}", &job_id.to_string()))
        .header("Authorization", format!("Bearer {}", token))
        .header("Content-Type", "application/json")
        .body(Body::from(
            serde_json::to_vec(&serde_json::json!({
                "tool_name": "missing_tool",
                "params": {}
            }))
            .expect("serialize remote-tool execute payload"),
        ))
        .expect("build remote-tool execute request");

    let resp = router
        .oneshot(req)
        .await
        .expect("send remote-tool execute request");
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

#[rstest]
#[tokio::test]
async fn remote_tool_execute_rejects_non_catalog_tools(test_state: OrchestratorState) {
    let status = execute_remote_tool_status(
        test_state,
        build_tool_fixture(ToolFixture::ContainerOnly),
        "remote_tool_execute_container",
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}

#[rstest]
#[tokio::test]
async fn remote_tool_execute_rejects_protected_orchestration_tools(test_state: OrchestratorState) {
    let status = execute_remote_tool_status(
        test_state,
        Arc::new(StubTool {
            name: "create_job",
            description: "Protected orchestration tool",
            output: StubOutput::Fixed(serde_json::json!({"created":true})),
            ..StubTool::hosted(
                "create_job",
                "",
                serde_json::json!({"type":"object","properties":{}}),
            )
        }),
        "create_job",
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}

#[rstest]
#[tokio::test]
async fn remote_tool_execute_rejects_approval_gated_tools(test_state: OrchestratorState) {
    let status = execute_remote_tool_status(
        test_state,
        build_tool_fixture(ToolFixture::ApprovalGated),
        "remote_tool_execute_gated",
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
}

#[rstest]
#[case(
    "remote_tool_execute_invalid_parameters",
    ExecuteErrorKind::InvalidParameters,
    StatusCode::BAD_REQUEST
)]
#[case(
    "remote_tool_execute_not_authorized",
    ExecuteErrorKind::NotAuthorized,
    StatusCode::FORBIDDEN
)]
#[case(
    "remote_tool_execute_rate_limited",
    ExecuteErrorKind::RateLimited,
    StatusCode::TOO_MANY_REQUESTS
)]
#[case(
    "remote_tool_execute_other_error",
    ExecuteErrorKind::ExecutionFailed,
    StatusCode::BAD_GATEWAY
)]
#[tokio::test]
async fn remote_tool_execute_maps_error_statuses(
    test_state: OrchestratorState,
    #[case] tool_name: &'static str,
    #[case] error_kind: ExecuteErrorKind,
    #[case] expected_status: StatusCode,
) {
    let status = execute_remote_tool_status(
        test_state,
        Arc::new(ErrorTool {
            name: tool_name,
            error_kind,
        }),
        tool_name,
    )
    .await;

    assert_eq!(status, expected_status);
}

#[rstest]
#[tokio::test]
async fn remote_tool_execute_propagates_request_job_id(test_state: OrchestratorState) {
    let seen_job_id = Arc::new(Mutex::new(None));
    test_state
        .tools
        .register(Arc::new(JobAwareTool {
            seen_job_id: Arc::clone(&seen_job_id),
        }))
        .await;
    let job_id = Uuid::new_v4();
    let token = test_state.token_store.create_token(job_id).await;
    let router = OrchestratorApi::router(test_state);

    let req = Request::builder()
        .method("POST")
        .uri(REMOTE_TOOL_EXECUTE_ROUTE.replace("{job_id}", &job_id.to_string()))
        .header("Authorization", format!("Bearer {}", token))
        .header("Content-Type", "application/json")
        .body(Body::from(
            serde_json::to_vec(&serde_json::json!({
                "tool_name": "remote_tool_execute_job_id",
                "params": {"query": "axinite"}
            }))
            .expect("serialize hosted remote-tool execute payload"),
        ))
        .expect("build hosted remote-tool execute request");

    let resp = router
        .oneshot(req)
        .await
        .expect("send hosted remote-tool execute request");
    assert_eq!(resp.status(), StatusCode::OK);

    let body = axum::body::to_bytes(resp.into_body(), 4096)
        .await
        .expect("read hosted remote-tool execute response body");
    let execute_resp: crate::worker::api::RemoteToolExecutionResponse =
        serde_json::from_slice(&body).expect("parse hosted remote-tool execute response");
    assert_eq!(execute_resp.output.result["echo"], "axinite");
    assert_eq!(*seen_job_id.lock().await, Some(job_id));
}
