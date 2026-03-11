//! Worker-local proxies for safe extension-management reads and activation.
//!
//! Hosted workers cannot consume interactive approval grants, so this module
//! only exposes the non-mutating extension tools that can be proxied through
//! the orchestrator without bypassing approval checks.

use std::sync::Arc;

use async_trait::async_trait;

use crate::context::JobContext;
use crate::tools::ToolRegistry;
use crate::tools::builtin::extension_tools::ExtensionToolKind;
use crate::tools::tool::{ApprovalRequirement, Tool, ToolError, ToolOutput};
use crate::worker::api::WorkerHttpClient;

struct WorkerExtensionProxyTool {
    kind: ExtensionToolKind,
    client: Arc<WorkerHttpClient>,
}

impl WorkerExtensionProxyTool {
    fn new(kind: ExtensionToolKind, client: Arc<WorkerHttpClient>) -> Self {
        Self { kind, client }
    }
}

#[async_trait]
impl Tool for WorkerExtensionProxyTool {
    fn name(&self) -> &str {
        self.kind.name()
    }

    fn description(&self) -> &str {
        self.kind.description()
    }

    fn parameters_schema(&self) -> serde_json::Value {
        self.kind.parameters_schema()
    }

    async fn execute(
        &self,
        params: serde_json::Value,
        _ctx: &JobContext,
    ) -> Result<ToolOutput, ToolError> {
        self.client
            .execute_extension_tool(self.kind.name(), &params)
            .await
            .map_err(|e| ToolError::ExecutionFailed(e.to_string()))
    }

    fn requires_approval(&self, _params: &serde_json::Value) -> ApprovalRequirement {
        self.kind.approval_requirement()
    }
}

pub(crate) fn register_worker_extension_proxy_tools(
    registry: &ToolRegistry,
    client: Arc<WorkerHttpClient>,
) {
    for kind in ExtensionToolKind::HOSTED_WORKER_PROXY_SAFE {
        registry.register_sync(Arc::new(WorkerExtensionProxyTool::new(
            kind,
            Arc::clone(&client),
        )));
    }
}

#[cfg(test)]
mod tests {
    use axum::extract::{Path, State};
    use axum::routing::post;
    use axum::{Json, Router};
    use rust_decimal::Decimal;
    use uuid::Uuid;

    use super::*;
    use crate::worker::api::{ProxyExtensionToolRequest, ProxyExtensionToolResponse};

    #[derive(Clone)]
    struct TestState;

    async fn execute_tool(
        State(_state): State<TestState>,
        Path(job_id): Path<Uuid>,
        Json(req): Json<ProxyExtensionToolRequest>,
    ) -> Json<ProxyExtensionToolResponse> {
        Json(ProxyExtensionToolResponse {
            output: ToolOutput::success(
                serde_json::json!({
                    "job_id": job_id,
                    "tool_name": req.tool_name,
                    "params": req.params,
                }),
                std::time::Duration::from_millis(7),
            )
            .with_cost(Decimal::new(125, 2))
            .with_raw("proxy raw output"),
        })
    }

    #[tokio::test]
    async fn proxy_tool_round_trips_extension_calls() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind listener");
        let addr = listener.local_addr().expect("listener addr");
        let router = Router::new()
            .route("/worker/{job_id}/extension_tool", post(execute_tool))
            .with_state(TestState);
        let server = tokio::spawn(async move {
            axum::serve(listener, router).await.expect("serve router");
        });

        let job_id = Uuid::new_v4();
        let client = Arc::new(WorkerHttpClient::new(
            format!("http://{}", addr),
            job_id,
            "test-token".to_string(),
        ));
        let registry = ToolRegistry::new();
        register_worker_extension_proxy_tools(&registry, client);

        let tool = registry
            .get("tool_list")
            .await
            .expect("tool_list proxy must be registered");
        let output = tool
            .execute(
                serde_json::json!({"include_available": true}),
                &JobContext::default(),
            )
            .await
            .expect("proxy execution should succeed");

        assert_eq!(output.result["tool_name"], "tool_list");
        assert_eq!(output.result["job_id"], job_id.to_string());
        assert_eq!(output.result["params"]["include_available"], true);
        assert_eq!(output.cost, Some(Decimal::new(125, 2)));
        assert_eq!(output.raw.as_deref(), Some("proxy raw output"));
        assert_eq!(output.duration, std::time::Duration::from_millis(7));

        server.abort();
        let _ = server.await;
    }

    #[tokio::test]
    async fn register_worker_extension_proxy_tools_excludes_approval_gated_tools() {
        let client = Arc::new(WorkerHttpClient::new(
            "http://127.0.0.1:1".to_string(),
            Uuid::new_v4(),
            "test-token".to_string(),
        ));
        let registry = ToolRegistry::new();

        register_worker_extension_proxy_tools(&registry, client);

        let mut names = registry.list().await;
        names.sort();

        assert_eq!(
            names,
            vec![
                "extension_info",
                "tool_activate",
                "tool_list",
                "tool_search"
            ]
        );
    }
}
