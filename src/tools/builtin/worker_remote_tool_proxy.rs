//! Worker-local proxies for orchestrator-owned hosted remote tools.
//!
//! Hosted workers keep their container-local development tools, but they fetch
//! orchestrator-owned hosted-visible tool definitions at startup and register
//! a local proxy for each one so the LLM sees the canonical metadata.

use std::sync::Arc;

use async_trait::async_trait;

use crate::context::JobContext;
use crate::llm::ToolDefinition;
use crate::tools::ToolRegistry;
use crate::tools::tool::{ApprovalRequirement, Tool, ToolError, ToolOutput};
use crate::worker::api::WorkerHttpClient;

struct WorkerRemoteToolProxy {
    definition: ToolDefinition,
    client: Arc<WorkerHttpClient>,
}

impl WorkerRemoteToolProxy {
    fn new(definition: ToolDefinition, client: Arc<WorkerHttpClient>) -> Self {
        Self { definition, client }
    }
}

#[async_trait]
impl Tool for WorkerRemoteToolProxy {
    fn name(&self) -> &str {
        &self.definition.name
    }

    fn description(&self) -> &str {
        &self.definition.description
    }

    fn parameters_schema(&self) -> serde_json::Value {
        self.definition.parameters.clone()
    }

    async fn execute(
        &self,
        params: serde_json::Value,
        _ctx: &JobContext,
    ) -> Result<ToolOutput, ToolError> {
        self.client
            .execute_remote_tool(&self.definition.name, &params)
            .await
            .map_err(|e| ToolError::ExecutionFailed(e.to_string()))
    }

    fn requires_approval(&self, _params: &serde_json::Value) -> ApprovalRequirement {
        ApprovalRequirement::Never
    }
}

pub(crate) fn register_worker_remote_tool_proxies(
    registry: &ToolRegistry,
    client: Arc<WorkerHttpClient>,
    definitions: Vec<ToolDefinition>,
) {
    for definition in definitions {
        registry.register_sync(Arc::new(WorkerRemoteToolProxy::new(
            definition,
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
    use crate::worker::api::{
        REMOTE_TOOL_EXECUTE_ROUTE, RemoteToolExecutionRequest, RemoteToolExecutionResponse,
    };

    #[derive(Clone)]
    struct TestState;

    async fn execute_tool(
        State(_state): State<TestState>,
        Path(job_id): Path<Uuid>,
        Json(req): Json<RemoteToolExecutionRequest>,
    ) -> Json<RemoteToolExecutionResponse> {
        Json(RemoteToolExecutionResponse {
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
    async fn remote_tool_execute_round_trips_catalog_tools() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind listener");
        let addr = listener.local_addr().expect("listener addr");
        let router = Router::new()
            .route(REMOTE_TOOL_EXECUTE_ROUTE, post(execute_tool))
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
        register_worker_remote_tool_proxies(
            &registry,
            client,
            vec![ToolDefinition {
                name: "github_search".to_string(),
                description: "Search repositories".to_string(),
                parameters: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "query": {"type": "string"}
                    },
                    "required": ["query"]
                }),
            }],
        );

        let tool = registry
            .get("github_search")
            .await
            .expect("github_search proxy must be registered");
        let output = tool
            .execute(
                serde_json::json!({"query": "axinite"}),
                &JobContext::default(),
            )
            .await
            .expect("proxy execution should succeed");

        assert_eq!(tool.name(), "github_search");
        assert_eq!(tool.description(), "Search repositories");
        assert_eq!(tool.parameters_schema()["required"][0], "query");
        assert_eq!(output.result["tool_name"], "github_search");
        assert_eq!(output.result["job_id"], job_id.to_string());
        assert_eq!(output.result["params"]["query"], "axinite");
        assert_eq!(output.cost, Some(Decimal::new(125, 2)));
        assert_eq!(output.raw.as_deref(), Some("proxy raw output"));
        assert_eq!(output.duration, std::time::Duration::from_millis(7));

        server.abort();
        let _ = server.await;
    }
}
