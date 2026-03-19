//! Tests for hosted worker proxy execution of extension-management tools.

use std::sync::Arc;
use std::time::Duration;

use rstest::rstest;

use super::fixtures::test_state;
use super::*;

struct HostedSafeActivateTool;

#[async_trait::async_trait]
impl Tool for HostedSafeActivateTool {
    fn name(&self) -> &str {
        "tool_activate"
    }

    fn description(&self) -> &str {
        "hosted-safe tool_activate"
    }

    fn parameters_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "name": { "type": "string" }
            },
            "required": ["name"]
        })
    }

    async fn execute(
        &self,
        params: serde_json::Value,
        _ctx: &crate::context::JobContext,
    ) -> Result<ToolOutput, crate::tools::ToolError> {
        Ok(ToolOutput::success(
            serde_json::json!({
                "activated": params["name"],
            }),
            Duration::from_millis(5),
        ))
    }

    fn requires_approval(&self, _params: &serde_json::Value) -> crate::tools::ApprovalRequirement {
        crate::tools::ApprovalRequirement::UnlessAutoApproved
    }
}

struct ApprovalAwareToolList;

#[async_trait::async_trait]
impl Tool for ApprovalAwareToolList {
    fn name(&self) -> &str {
        "tool_list"
    }

    fn description(&self) -> &str {
        "approval-aware tool_list"
    }

    fn parameters_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "require_approval": { "type": "boolean" }
            }
        })
    }

    async fn execute(
        &self,
        _params: serde_json::Value,
        _ctx: &crate::context::JobContext,
    ) -> Result<ToolOutput, crate::tools::ToolError> {
        panic!("approval-gated proxy requests must not execute")
    }

    fn requires_approval(&self, params: &serde_json::Value) -> crate::tools::ApprovalRequirement {
        if params["require_approval"].as_bool() == Some(true) {
            crate::tools::ApprovalRequirement::Always
        } else {
            crate::tools::ApprovalRequirement::Never
        }
    }
}

struct FakeToolList {
    seen_job_id: Arc<tokio::sync::Mutex<Option<Uuid>>>,
}

#[async_trait::async_trait]
impl Tool for FakeToolList {
    fn name(&self) -> &str {
        "tool_list"
    }

    fn description(&self) -> &str {
        "fake tool_list"
    }

    fn parameters_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {}
        })
    }

    async fn execute(
        &self,
        _params: serde_json::Value,
        ctx: &crate::context::JobContext,
    ) -> Result<ToolOutput, crate::tools::ToolError> {
        *self.seen_job_id.lock().await = Some(ctx.job_id);
        Ok(ToolOutput::success(
            serde_json::json!({"extensions": ["telegram"]}),
            Duration::from_millis(5),
        ))
    }
}

#[rstest]
#[tokio::test]
async fn extension_tool_proxy_rejects_non_extension_tool_names(test_state: OrchestratorState) {
    let job_id = Uuid::new_v4();
    let token = test_state.token_store.create_token(job_id).await;
    let router = OrchestratorApi::router(test_state);

    let payload = serde_json::json!({
        "tool_name": "shell",
        "params": {"command": "ls"}
    });

    let req = Request::builder()
        .method("POST")
        .uri(format!("/worker/{}/extension_tool", job_id))
        .header("Authorization", format!("Bearer {}", token))
        .header("Content-Type", "application/json")
        .body(Body::from(
            serde_json::to_vec(&payload).expect("serialize proxy extension tool payload"),
        ))
        .expect("build proxy extension tool request");

    let resp = router
        .oneshot(req)
        .await
        .expect("send proxy extension tool request");
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
}

#[rstest]
#[tokio::test]
async fn extension_tool_proxy_rejects_extension_tools_that_require_approval_for_params(
    test_state: OrchestratorState,
) {
    test_state
        .tools
        .register(Arc::new(ApprovalAwareToolList))
        .await;
    let job_id = Uuid::new_v4();
    let token = test_state.token_store.create_token(job_id).await;
    let router = OrchestratorApi::router(test_state);

    let payload = serde_json::json!({
        "tool_name": "tool_list",
        "params": {"require_approval": true}
    });

    let req = Request::builder()
        .method("POST")
        .uri(format!("/worker/{}/extension_tool", job_id))
        .header("Authorization", format!("Bearer {}", token))
        .header("Content-Type", "application/json")
        .body(Body::from(
            serde_json::to_vec(&payload).expect("serialize approval-gated proxy payload"),
        ))
        .expect("serialize and build approval-gated request body");

    let resp = router
        .oneshot(req)
        .await
        .expect("router oneshot failed for approval-gated request");
    assert_eq!(resp.status(), StatusCode::FORBIDDEN);
}

#[rstest]
#[tokio::test]
async fn extension_tool_proxy_allows_hosted_safe_tools_with_unless_auto_approved(
    test_state: OrchestratorState,
) {
    test_state
        .tools
        .register(Arc::new(HostedSafeActivateTool))
        .await;
    let job_id = Uuid::new_v4();
    let token = test_state.token_store.create_token(job_id).await;
    let router = OrchestratorApi::router(test_state);

    let payload = serde_json::json!({
        "tool_name": "tool_activate",
        "params": {"name": "slack"}
    });

    let req = Request::builder()
        .method("POST")
        .uri(format!("/worker/{}/extension_tool", job_id))
        .header("Authorization", format!("Bearer {}", token))
        .header("Content-Type", "application/json")
        .body(Body::from(
            serde_json::to_vec(&payload).expect("serialize hosted-safe activate payload"),
        ))
        .expect("build hosted-safe activate request");

    let resp = router
        .oneshot(req)
        .await
        .expect("send hosted-safe activate request");
    assert_eq!(resp.status(), StatusCode::OK);

    let body = axum::body::to_bytes(resp.into_body(), 4096)
        .await
        .expect("read hosted-safe activate response");
    let proxy_resp: crate::worker::api::ProxyExtensionToolResponse =
        serde_json::from_slice(&body).expect("parse hosted-safe activate response");
    assert_eq!(proxy_resp.output.result["activated"], "slack");
}

#[rstest]
#[tokio::test]
async fn extension_tool_proxy_executes_registered_extension_tool_with_request_job_id(
    test_state: OrchestratorState,
) {
    let seen_job_id = Arc::new(tokio::sync::Mutex::new(None));
    test_state.tools.register_sync(Arc::new(FakeToolList {
        seen_job_id: Arc::clone(&seen_job_id),
    }));
    let job_id = Uuid::new_v4();
    let token = test_state.token_store.create_token(job_id).await;
    let router = OrchestratorApi::router(test_state);

    let payload = serde_json::json!({
        "tool_name": "tool_list",
        "params": {"include_available": true}
    });

    let req = Request::builder()
        .method("POST")
        .uri(format!("/worker/{}/extension_tool", job_id))
        .header("Authorization", format!("Bearer {}", token))
        .header("Content-Type", "application/json")
        .body(Body::from(
            serde_json::to_vec(&payload).expect("serialize registered proxy payload"),
        ))
        .expect("build registered proxy extension tool request");

    let resp = router
        .oneshot(req)
        .await
        .expect("send registered proxy extension tool request");
    assert_eq!(resp.status(), StatusCode::OK);

    let body = axum::body::to_bytes(resp.into_body(), 4096)
        .await
        .expect("read registered proxy extension tool response body");
    let proxy_resp: crate::worker::api::ProxyExtensionToolResponse =
        serde_json::from_slice(&body).expect("parse registered proxy extension tool response");
    assert_eq!(proxy_resp.output.result["extensions"][0], "telegram");
    assert_eq!(proxy_resp.output.duration, Duration::from_millis(5));
    assert_eq!(proxy_resp.output.cost, None);
    assert_eq!(proxy_resp.output.raw, None);
    assert_eq!(*seen_job_id.lock().await, Some(job_id));
}
