//! Tests for hosted remote-tool catalog fetch and execution.

use std::sync::Arc;

use rstest::rstest;

use super::super::remote_tools::hosted_remote_tool_catalog;
use super::fixtures::remote_tool_helpers::execute_remote_tool_status;
use super::fixtures::remote_tool_mocks::{
    ErrorTool, ExecuteErrorKind, JobAwareTool, approval_gated_tool, container_only_tool,
    hosted_catalog_tool, hosted_catalog_tool_beta, protected_job_events_tool,
    protected_orchestration_tool,
};
use super::fixtures::test_state;
use super::*;
use crate::worker::api::{REMOTE_TOOL_CATALOG_ROUTE, REMOTE_TOOL_EXECUTE_ROUTE};

#[rstest]
#[tokio::test]
async fn remote_tool_catalog_returns_hosted_safe_tool_definitions(test_state: OrchestratorState) {
    test_state
        .tools
        .register(Arc::new(hosted_catalog_tool()))
        .await;
    test_state
        .tools
        .register(Arc::new(protected_orchestration_tool()))
        .await;
    test_state
        .tools
        .register(Arc::new(protected_job_events_tool()))
        .await;
    test_state
        .tools
        .register(Arc::new(approval_gated_tool()))
        .await;
    test_state
        .tools
        .register(Arc::new(container_only_tool()))
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
            "properties": {
                "query": {
                    "type": "string",
                    "description": "search query"
                }
            },
            "required": ["query"]
        })
    );
}

#[rstest]
#[tokio::test]
async fn remote_tool_catalog_excludes_job_events_named_tools() {
    let registry = Arc::new(ToolRegistry::new());
    registry.register(Arc::new(hosted_catalog_tool())).await;
    registry
        .register(Arc::new(protected_job_events_tool()))
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

#[rstest]
#[tokio::test]
async fn remote_tool_catalog_sorts_tools_before_versioning() {
    let registry_a = Arc::new(ToolRegistry::new());
    registry_a
        .register(Arc::new(hosted_catalog_tool_beta()))
        .await;
    registry_a.register(Arc::new(hosted_catalog_tool())).await;

    let registry_b = Arc::new(ToolRegistry::new());
    registry_b.register(Arc::new(hosted_catalog_tool())).await;
    registry_b
        .register(Arc::new(hosted_catalog_tool_beta()))
        .await;

    let (tools_a, instructions_a, version_a) = hosted_remote_tool_catalog(&registry_a).await;
    let (tools_b, instructions_b, version_b) = hosted_remote_tool_catalog(&registry_b).await;

    assert_eq!(
        tools_a
            .iter()
            .map(|tool| tool.name.as_str())
            .collect::<Vec<_>>(),
        vec![
            "remote_tool_catalog_fixture",
            "remote_tool_catalog_fixture_beta"
        ]
    );
    assert_eq!(
        tools_b
            .iter()
            .map(|tool| tool.name.as_str())
            .collect::<Vec<_>>(),
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
    test_state
        .tools
        .register(Arc::new(container_only_tool()))
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
                "tool_name": "remote_tool_execute_container",
                "params": {}
            }))
            .expect("serialize non-catalog remote-tool execute payload"),
        ))
        .expect("build non-catalog remote-tool execute request");

    let resp = router
        .oneshot(req)
        .await
        .expect("send non-catalog remote-tool execute request");
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
}

#[rstest]
#[tokio::test]
async fn remote_tool_execute_rejects_protected_orchestration_tools(test_state: OrchestratorState) {
    test_state
        .tools
        .register(Arc::new(protected_orchestration_tool()))
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
                "tool_name": "create_job",
                "params": {}
            }))
            .expect("serialize protected orchestration remote-tool execute payload"),
        ))
        .expect("build protected orchestration remote-tool execute request");

    let resp = router
        .oneshot(req)
        .await
        .expect("send protected orchestration remote-tool execute request");
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
}

#[rstest]
#[tokio::test]
async fn remote_tool_execute_rejects_approval_gated_tools(test_state: OrchestratorState) {
    test_state
        .tools
        .register(Arc::new(approval_gated_tool()))
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
                "tool_name": "remote_tool_execute_gated",
                "params": {}
            }))
            .expect("serialize approval-gated remote-tool execute payload"),
        ))
        .expect("build approval-gated remote-tool execute request");

    let resp = router
        .oneshot(req)
        .await
        .expect("send approval-gated remote-tool execute request");
    assert_eq!(resp.status(), StatusCode::FORBIDDEN);
}

#[rstest]
#[tokio::test]
async fn remote_tool_execute_returns_400_on_invalid_parameters(test_state: OrchestratorState) {
    let status = execute_remote_tool_status(
        test_state,
        Arc::new(ErrorTool {
            name: "remote_tool_execute_invalid_parameters",
            error_kind: ExecuteErrorKind::InvalidParameters,
        }),
        "remote_tool_execute_invalid_parameters",
    )
    .await;

    assert_eq!(status, StatusCode::BAD_REQUEST);
}

#[rstest]
#[tokio::test]
async fn remote_tool_execute_returns_403_on_not_authorized(test_state: OrchestratorState) {
    let status = execute_remote_tool_status(
        test_state,
        Arc::new(ErrorTool {
            name: "remote_tool_execute_not_authorized",
            error_kind: ExecuteErrorKind::NotAuthorized,
        }),
        "remote_tool_execute_not_authorized",
    )
    .await;

    assert_eq!(status, StatusCode::FORBIDDEN);
}

#[rstest]
#[tokio::test]
async fn remote_tool_execute_returns_429_on_rate_limited(test_state: OrchestratorState) {
    let status = execute_remote_tool_status(
        test_state,
        Arc::new(ErrorTool {
            name: "remote_tool_execute_rate_limited",
            error_kind: ExecuteErrorKind::RateLimited,
        }),
        "remote_tool_execute_rate_limited",
    )
    .await;

    assert_eq!(status, StatusCode::TOO_MANY_REQUESTS);
}

#[rstest]
#[tokio::test]
async fn remote_tool_execute_returns_502_on_other_errors(test_state: OrchestratorState) {
    let status = execute_remote_tool_status(
        test_state,
        Arc::new(ErrorTool {
            name: "remote_tool_execute_other_error",
            error_kind: ExecuteErrorKind::ExecutionFailed,
        }),
        "remote_tool_execute_other_error",
    )
    .await;

    assert_eq!(status, StatusCode::BAD_GATEWAY);
}

#[rstest]
#[tokio::test]
async fn remote_tool_execute_propagates_request_job_id(test_state: OrchestratorState) {
    let seen_job_id = Arc::new(Mutex::new(None));
    test_state.tools.register_sync(Arc::new(JobAwareTool {
        seen_job_id: Arc::clone(&seen_job_id),
    }));
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
