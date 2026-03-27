//! Worker-local proxies for hosted-safe extension-management tools.
//!
//! Hosted workers cannot consume interactive approval grants, so this module
//! only exposes the explicitly allowlisted extension tools that can be proxied
//! through the orchestrator without requiring an interactive approval flow.

use std::sync::Arc;

use crate::context::JobContext;
use crate::error::WorkerError;
use crate::llm::ToolDefinition;
use crate::tools::ToolRegistry;
use crate::tools::tool::{ApprovalRequirement, NativeTool, ToolError, ToolOutput};
use crate::worker::api::WorkerHttpClient;

impl NativeTool for WorkerRemoteToolProxy {
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
            .map_err(map_worker_error_to_tool_error)
    }

    fn requires_approval(&self, _params: &serde_json::Value) -> ApprovalRequirement {
        ApprovalRequirement::Never
    }
}

fn map_worker_error_to_tool_error(error: WorkerError) -> ToolError {
    match error {
        WorkerError::BadRequest { reason } => ToolError::InvalidParameters(reason),
        WorkerError::Unauthorized { reason } => ToolError::NotAuthorized(reason),
        WorkerError::RateLimited { retry_after, .. } => ToolError::RateLimited(retry_after),
        WorkerError::BadGateway { reason } => ToolError::ExternalService(reason),
        WorkerError::RemoteToolFailed { reason } => ToolError::ExternalService(reason),
        other => ToolError::ExecutionFailed(other.to_string()),
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

struct WorkerRemoteToolProxy {
    definition: ToolDefinition,
    client: Arc<WorkerHttpClient>,
}

impl WorkerRemoteToolProxy {
    fn new(definition: ToolDefinition, client: Arc<WorkerHttpClient>) -> Self {
        Self { definition, client }
    }
}

#[cfg(test)]
mod tests;
