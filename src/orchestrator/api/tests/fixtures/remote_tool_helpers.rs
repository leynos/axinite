//! Shared helper functions for hosted remote-tool endpoint tests.

use std::sync::Arc;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use tower::ServiceExt;
use uuid::Uuid;

use crate::orchestrator::api::{OrchestratorApi, OrchestratorState};
use crate::tools::Tool;
use crate::worker::api::REMOTE_TOOL_EXECUTE_ROUTE;

pub(crate) async fn execute_remote_tool_status(
    test_state: OrchestratorState,
    tool: Arc<dyn Tool>,
    tool_name: &str,
) -> StatusCode {
    if crate::tools::ToolRegistry::is_protected_tool_name(tool.name()) {
        test_state.tools.register_sync(Arc::clone(&tool));
    } else {
        test_state.tools.register(tool).await;
    }
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
                "tool_name": tool_name,
                "params": {}
            }))
            .expect("serialize hosted remote-tool execute payload"),
        ))
        .expect("build hosted remote-tool execute request");

    router
        .oneshot(req)
        .await
        .expect("send hosted remote-tool execute request")
        .status()
}
