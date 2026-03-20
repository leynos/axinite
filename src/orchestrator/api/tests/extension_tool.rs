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
    seen_params: Arc<tokio::sync::Mutex<Option<serde_json::Value>>>,
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
        params: serde_json::Value,
        ctx: &crate::context::JobContext,
    ) -> Result<ToolOutput, crate::tools::ToolError> {
        *self.seen_job_id.lock().await = Some(ctx.job_id);
        *self.seen_params.lock().await = Some(params.clone());
        Ok(ToolOutput::success(
            serde_json::json!({"extensions": ["telegram"]}),
            Duration::from_millis(5),
        ))
    }
}

#[derive(Debug, Clone, Copy)]
enum ExtensionToolSuccessKind {
    HostedSafeActivate,
    RegisteredToolList,
}

#[derive(Debug)]
struct ExtensionToolSuccessCase {
    kind: ExtensionToolSuccessKind,
    payload: serde_json::Value,
    expected_key: &'static str,
    expected_value: &'static str,
    expected_params: Option<serde_json::Value>,
}

fn hosted_safe_activate_case() -> ExtensionToolSuccessCase {
    ExtensionToolSuccessCase {
        kind: ExtensionToolSuccessKind::HostedSafeActivate,
        payload: serde_json::json!({
            "tool_name": "tool_activate",
            "params": {"name": "slack"}
        }),
        expected_key: "activated",
        expected_value: "slack",
        expected_params: None,
    }
}

fn registered_tool_list_case() -> ExtensionToolSuccessCase {
    ExtensionToolSuccessCase {
        kind: ExtensionToolSuccessKind::RegisteredToolList,
        payload: serde_json::json!({
            "tool_name": "tool_list",
            "params": {"include_available": true}
        }),
        expected_key: "extensions",
        expected_value: "telegram",
        expected_params: Some(serde_json::json!({"include_available": true})),
    }
}

struct RegisteredToolObservations {
    seen_job_id: Arc<tokio::sync::Mutex<Option<Uuid>>>,
    seen_params: Arc<tokio::sync::Mutex<Option<serde_json::Value>>>,
}

async fn register_extension_tool_case(
    test_state: &OrchestratorState,
    kind: ExtensionToolSuccessKind,
) -> Option<RegisteredToolObservations> {
    match kind {
        ExtensionToolSuccessKind::HostedSafeActivate => {
            test_state
                .tools
                .register(Arc::new(HostedSafeActivateTool))
                .await;
            None
        }
        ExtensionToolSuccessKind::RegisteredToolList => {
            let seen_job_id = Arc::new(tokio::sync::Mutex::new(None));
            let seen_params = Arc::new(tokio::sync::Mutex::new(None));
            test_state.tools.register_sync(Arc::new(FakeToolList {
                seen_job_id: Arc::clone(&seen_job_id),
                seen_params: Arc::clone(&seen_params),
            }));
            Some(RegisteredToolObservations {
                seen_job_id,
                seen_params,
            })
        }
    }
}

async fn post_extension_tool(
    router: Router,
    job_id: Uuid,
    token: &str,
    payload: serde_json::Value,
) -> anyhow::Result<axum::http::Response<Body>> {
    let req = Request::builder()
        .method("POST")
        .uri(format!("/worker/{job_id}/extension_tool"))
        .header("Authorization", format!("Bearer {token}"))
        .header("Content-Type", "application/json")
        .body(Body::from(serde_json::to_vec(&payload)?))?;

    router.oneshot(req).await.map_err(anyhow::Error::from)
}

async fn decode_proxy_extension_tool_response(
    resp: axum::http::Response<Body>,
) -> anyhow::Result<crate::worker::api::ProxyExtensionToolResponse> {
    let body = axum::body::to_bytes(resp.into_body(), 4096).await?;
    Ok(serde_json::from_slice(&body)?)
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
#[case(hosted_safe_activate_case())]
#[case(registered_tool_list_case())]
#[tokio::test]
async fn extension_tool_proxy_executes_hosted_visible_extension_tools(
    test_state: OrchestratorState,
    #[case] case: ExtensionToolSuccessCase,
) -> anyhow::Result<()> {
    let seen_job_id = register_extension_tool_case(&test_state, case.kind).await;
    let job_id = Uuid::new_v4();
    let token = test_state.token_store.create_token(job_id).await;
    let router = OrchestratorApi::router(test_state);

    let resp = post_extension_tool(router, job_id, &token, case.payload).await?;
    assert_eq!(resp.status(), StatusCode::OK);

    let proxy_resp = decode_proxy_extension_tool_response(resp).await?;
    let result = &proxy_resp.output.result;
    let actual = match case.expected_key {
        "extensions" => &result[case.expected_key][0],
        _ => &result[case.expected_key],
    };
    assert_eq!(actual, case.expected_value);
    assert_eq!(proxy_resp.output.duration, Duration::from_millis(5));
    assert_eq!(proxy_resp.output.cost, None);
    assert_eq!(proxy_resp.output.raw, None);

    if let Some(observations) = seen_job_id {
        assert_eq!(*observations.seen_job_id.lock().await, Some(job_id));
        assert_eq!(*observations.seen_params.lock().await, case.expected_params,);
    }

    Ok(())
}
