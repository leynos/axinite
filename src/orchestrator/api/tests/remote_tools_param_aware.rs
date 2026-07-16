//! Focused tests for hosted remote-tool approval logic that depends on params.

use std::sync::Arc;

use axum::body::Body;
use axum::http::Request;
use rstest::rstest;
use tower::ServiceExt;
use uuid::Uuid;

use super::super::remote_tools::hosted_remote_tool_catalog;
use super::fixtures::remote_tool_mocks::ParamAwareHostedTool;
use super::fixtures::test_state;
use super::*;
use crate::worker::api::REMOTE_TOOL_EXECUTE_ROUTE;

#[rstest]
#[tokio::test]
async fn remote_tool_catalog_uses_explicit_hosted_eligibility() {
    let registry = Arc::new(ToolRegistry::new());
    registry.register(Arc::new(ParamAwareHostedTool)).await;

    let (tools, _instructions, _version) = hosted_remote_tool_catalog(&registry).await;

    assert_eq!(
        tools
            .iter()
            .map(|tool| tool.name.as_str())
            .collect::<Vec<_>>(),
        vec!["remote_tool_execute_param_aware"]
    );
}

#[rstest]
#[tokio::test]
async fn remote_tool_execute_rejects_param_dependent_approval_requests(
    test_state: OrchestratorState,
) {
    test_state
        .tools
        .register(Arc::new(ParamAwareHostedTool))
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
                "tool_name": "remote_tool_execute_param_aware",
                "params": {"dangerous": true}
            }))
            .expect("serialize param-aware remote-tool execute payload"),
        ))
        .expect("build param-aware remote-tool execute request");

    let resp = router
        .oneshot(req)
        .await
        .expect("send param-aware remote-tool execute request");
    assert_eq!(resp.status(), StatusCode::FORBIDDEN);
}
