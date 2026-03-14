//! Tests for hosted remote-tool catalog fetch and execution.

use std::sync::Arc;
use std::time::Duration;

use rstest::rstest;

use super::fixtures::test_state;
use super::*;
use crate::tools::{ApprovalRequirement, ToolDomain, ToolError};
use crate::worker::api::{REMOTE_TOOL_CATALOG_ROUTE, REMOTE_TOOL_EXECUTE_ROUTE};

struct HostedCatalogTool;

#[async_trait::async_trait]
impl Tool for HostedCatalogTool {
    fn name(&self) -> &str {
        "remote_tool_catalog_fixture"
    }

    fn description(&self) -> &str {
        "Hosted-safe tool for catalog tests"
    }

    fn parameters_schema(&self) -> serde_json::Value {
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
    }

    async fn execute(
        &self,
        params: serde_json::Value,
        _ctx: &crate::context::JobContext,
    ) -> Result<ToolOutput, ToolError> {
        Ok(ToolOutput::success(params, Duration::from_millis(5)))
    }
}

struct ApprovalGatedTool;

#[async_trait::async_trait]
impl Tool for ApprovalGatedTool {
    fn name(&self) -> &str {
        "remote_tool_execute_gated"
    }

    fn description(&self) -> &str {
        "Approval-gated tool"
    }

    fn parameters_schema(&self) -> serde_json::Value {
        serde_json::json!({"type": "object", "properties": {}})
    }

    async fn execute(
        &self,
        _params: serde_json::Value,
        _ctx: &crate::context::JobContext,
    ) -> Result<ToolOutput, ToolError> {
        panic!("approval-gated tool must not execute");
    }

    fn requires_approval(&self, _params: &serde_json::Value) -> ApprovalRequirement {
        ApprovalRequirement::Always
    }
}

struct ContainerOnlyTool;

#[async_trait::async_trait]
impl Tool for ContainerOnlyTool {
    fn name(&self) -> &str {
        "remote_tool_execute_container"
    }

    fn description(&self) -> &str {
        "Container-only tool"
    }

    fn parameters_schema(&self) -> serde_json::Value {
        serde_json::json!({"type": "object", "properties": {}})
    }

    async fn execute(
        &self,
        _params: serde_json::Value,
        _ctx: &crate::context::JobContext,
    ) -> Result<ToolOutput, ToolError> {
        panic!("container-only tool must not execute");
    }

    fn domain(&self) -> ToolDomain {
        ToolDomain::Container
    }
}

struct JobAwareTool {
    seen_job_id: Arc<Mutex<Option<Uuid>>>,
}

#[async_trait::async_trait]
impl Tool for JobAwareTool {
    fn name(&self) -> &str {
        "remote_tool_execute_job_id"
    }

    fn description(&self) -> &str {
        "Captures the request job id"
    }

    fn parameters_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "query": {"type": "string"}
            },
            "required": ["query"]
        })
    }

    async fn execute(
        &self,
        params: serde_json::Value,
        ctx: &crate::context::JobContext,
    ) -> Result<ToolOutput, ToolError> {
        *self.seen_job_id.lock().await = Some(ctx.job_id);
        Ok(ToolOutput::success(
            serde_json::json!({"echo": params["query"]}),
            Duration::from_millis(5),
        ))
    }
}

#[rstest]
#[tokio::test]
async fn remote_tool_catalog_returns_hosted_safe_tool_definitions(test_state: OrchestratorState) {
    test_state.tools.register(Arc::new(HostedCatalogTool)).await;
    test_state.tools.register(Arc::new(ApprovalGatedTool)).await;
    test_state.tools.register(Arc::new(ContainerOnlyTool)).await;
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
    test_state.tools.register(Arc::new(ContainerOnlyTool)).await;
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
async fn remote_tool_execute_rejects_approval_gated_tools(test_state: OrchestratorState) {
    test_state.tools.register(Arc::new(ApprovalGatedTool)).await;
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
