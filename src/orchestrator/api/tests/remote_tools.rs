//! Tests for hosted remote-tool catalogue fetch and execution.

use std::sync::Arc;

use rstest::rstest;

use super::super::remote_tools::hosted_remote_tool_catalog;
use super::fixtures::remote_tool_helpers::execute_remote_tool_status;
use super::fixtures::remote_tool_mocks::{
    ErrorTool, ExecuteErrorKind, JobAwareTool, StubOutput, StubTool, ToolFixture,
    build_tool_fixture, complex_tool_definition, complex_tool_stub,
};
use super::fixtures::test_state;
use super::*;
use crate::tools::HostedToolCatalogSource;
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
            name: "hosted_extension_catalog_builtin",
            description: "Hosted-safe extension-management built-in",
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

fn expected_catalog_fixture_definition() -> crate::llm::ToolDefinition {
    crate::llm::ToolDefinition {
        name: "remote_tool_catalog_fixture".to_string(),
        description: "Hosted-safe tool for catalog tests".to_string(),
        parameters: serde_json::json!({
            "type": "object",
            "properties": {"query": {"type": "string", "description": "search query"}},
            "required": ["query"]
        }),
    }
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
    let expected = expected_catalog_fixture_definition();
    assert_eq!(tool.name, expected.name);
    assert_eq!(tool.description, expected.description);
    assert_eq!(tool.parameters, expected.parameters);
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
        description,
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
async fn orchestrator_responses_deserialize_into_worker_shared_types() {
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

    assert_eq!(deserialized.tools.len(), tools.len());
    assert_eq!(deserialized.tools[0], tools[0]);
    assert_eq!(deserialized.toolset_instructions, instructions);
    assert_eq!(deserialized.catalog_version, version);

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
