//! Tests for the hosted remote-tool execute endpoint: eligibility checks,
//! error-status mapping, and job-id propagation.

use std::sync::Arc;

use rstest::rstest;

use super::super::fixtures::remote_tool_helpers::execute_remote_tool_status;
use super::super::fixtures::remote_tool_mocks::{
    ErrorTool, ExecuteErrorKind, JobAwareTool, StubOutput, StubTool, ToolFixture,
    build_tool_fixture,
};
use super::super::fixtures::test_state;
use super::super::*;
use crate::worker::api::REMOTE_TOOL_EXECUTE_ROUTE;

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
            description: "Protected orchestration tool".to_string(),
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
#[tokio::test]
async fn remote_tool_execute_allows_hosted_wasm_tools(test_state: OrchestratorState) {
    let status = execute_remote_tool_status(
        test_state,
        build_tool_fixture(ToolFixture::CatalogWasm),
        "remote_tool_catalog_fixture_wasm",
    )
    .await;
    assert_eq!(status, StatusCode::OK);
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
