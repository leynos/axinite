//! Shared mock tools for hosted remote-tool endpoint tests.

use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use tokio::sync::Mutex;
use uuid::Uuid;

use crate::context::JobContext;
use crate::tools::{
    ApprovalRequirement, HostedToolEligibility, Tool, ToolDomain, ToolError, ToolOutput,
};

pub(crate) struct HostedCatalogTool;

#[async_trait]
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
        _ctx: &JobContext,
    ) -> Result<ToolOutput, ToolError> {
        Ok(ToolOutput::success(params, Duration::from_millis(5)))
    }
}

pub(crate) struct HostedCatalogToolBeta;

#[async_trait]
impl Tool for HostedCatalogToolBeta {
    fn name(&self) -> &str {
        "remote_tool_catalog_fixture_beta"
    }

    fn description(&self) -> &str {
        "Second hosted-safe tool for catalog tests"
    }

    fn parameters_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "path": {
                    "type": "string"
                }
            },
            "required": ["path"]
        })
    }

    async fn execute(
        &self,
        params: serde_json::Value,
        _ctx: &JobContext,
    ) -> Result<ToolOutput, ToolError> {
        Ok(ToolOutput::success(params, Duration::from_millis(5)))
    }
}

pub(crate) struct ProtectedOrchestrationTool;

#[async_trait]
impl Tool for ProtectedOrchestrationTool {
    fn name(&self) -> &str {
        "create_job"
    }

    fn description(&self) -> &str {
        "Protected orchestration tool"
    }

    fn parameters_schema(&self) -> serde_json::Value {
        serde_json::json!({"type": "object", "properties": {}})
    }

    async fn execute(
        &self,
        _params: serde_json::Value,
        _ctx: &JobContext,
    ) -> Result<ToolOutput, ToolError> {
        Ok(ToolOutput::success(
            serde_json::json!({"created": true}),
            Duration::from_millis(5),
        ))
    }
}

pub(crate) struct ProtectedJobEventsTool;

#[async_trait]
impl Tool for ProtectedJobEventsTool {
    fn name(&self) -> &str {
        "job_events"
    }

    fn description(&self) -> &str {
        "Protected job-events tool"
    }

    fn parameters_schema(&self) -> serde_json::Value {
        serde_json::json!({"type": "object", "properties": {}})
    }

    async fn execute(
        &self,
        _params: serde_json::Value,
        _ctx: &JobContext,
    ) -> Result<ToolOutput, ToolError> {
        Ok(ToolOutput::success(
            serde_json::json!({"events": []}),
            Duration::from_millis(5),
        ))
    }
}

pub(crate) struct ApprovalGatedTool;

#[async_trait]
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
        _ctx: &JobContext,
    ) -> Result<ToolOutput, ToolError> {
        panic!("approval-gated tool must not execute");
    }

    fn requires_approval(&self, _params: &serde_json::Value) -> ApprovalRequirement {
        ApprovalRequirement::Always
    }

    fn hosted_tool_eligibility(&self) -> HostedToolEligibility {
        HostedToolEligibility::ApprovalGated
    }
}

pub(crate) struct ParamAwareHostedTool;

#[async_trait]
impl Tool for ParamAwareHostedTool {
    fn name(&self) -> &str {
        "remote_tool_execute_param_aware"
    }

    fn description(&self) -> &str {
        "Hosted-safe tool with param-dependent approval"
    }

    fn parameters_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "dangerous": {"type": "boolean", "default": false}
            }
        })
    }

    async fn execute(
        &self,
        params: serde_json::Value,
        _ctx: &JobContext,
    ) -> Result<ToolOutput, ToolError> {
        Ok(ToolOutput::success(
            serde_json::json!({"dangerous": params["dangerous"]}),
            Duration::from_millis(5),
        ))
    }

    fn requires_approval(&self, params: &serde_json::Value) -> ApprovalRequirement {
        if params
            .get("dangerous")
            .and_then(serde_json::Value::as_bool)
            .unwrap_or(false)
        {
            ApprovalRequirement::Always
        } else {
            ApprovalRequirement::Never
        }
    }

    fn hosted_tool_eligibility(&self) -> HostedToolEligibility {
        HostedToolEligibility::Eligible
    }
}

pub(crate) struct ContainerOnlyTool;

#[async_trait]
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
        _ctx: &JobContext,
    ) -> Result<ToolOutput, ToolError> {
        panic!("container-only tool must not execute");
    }

    fn domain(&self) -> ToolDomain {
        ToolDomain::Container
    }
}

pub(crate) struct JobAwareTool {
    pub(crate) seen_job_id: Arc<Mutex<Option<Uuid>>>,
}

#[async_trait]
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
        ctx: &JobContext,
    ) -> Result<ToolOutput, ToolError> {
        *self.seen_job_id.lock().await = Some(ctx.job_id);
        Ok(ToolOutput::success(
            serde_json::json!({"echo": params["query"]}),
            Duration::from_millis(5),
        ))
    }
}

pub(crate) enum ExecuteErrorKind {
    InvalidParameters,
    NotAuthorized,
    RateLimited,
    ExecutionFailed,
}

pub(crate) struct ErrorTool {
    pub(crate) name: &'static str,
    pub(crate) error_kind: ExecuteErrorKind,
}

#[async_trait]
impl Tool for ErrorTool {
    fn name(&self) -> &str {
        self.name
    }

    fn description(&self) -> &str {
        "Returns a fixed execution error"
    }

    fn parameters_schema(&self) -> serde_json::Value {
        serde_json::json!({"type": "object", "properties": {}})
    }

    async fn execute(
        &self,
        _params: serde_json::Value,
        _ctx: &JobContext,
    ) -> Result<ToolOutput, ToolError> {
        Err(match self.error_kind {
            ExecuteErrorKind::InvalidParameters => ToolError::InvalidParameters("bad".to_string()),
            ExecuteErrorKind::NotAuthorized => ToolError::NotAuthorized("nope".to_string()),
            ExecuteErrorKind::RateLimited => ToolError::RateLimited(None),
            ExecuteErrorKind::ExecutionFailed => ToolError::ExecutionFailed("boom".to_string()),
        })
    }
}
