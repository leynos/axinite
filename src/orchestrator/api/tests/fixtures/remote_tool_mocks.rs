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

/// Output behaviour for [`StubTool`].
pub(crate) enum StubOutput {
    /// Echo the incoming `params` value back as the output.
    EchoParams,
    /// Return a fixed `serde_json::Value` regardless of `params`.
    Fixed(serde_json::Value),
    /// Panic with the given message (for tools that must never be executed).
    Panic(&'static str),
}

/// General-purpose parameterised stub implementing [`Tool`].
pub(crate) struct StubTool {
    pub(crate) name: &'static str,
    pub(crate) description: &'static str,
    pub(crate) parameters: serde_json::Value,
    pub(crate) domain: ToolDomain,
    pub(crate) always_approve: bool,
    pub(crate) eligibility: HostedToolEligibility,
    pub(crate) output: StubOutput,
}

impl StubTool {
    pub(crate) fn hosted(
        name: &'static str,
        description: &'static str,
        parameters: serde_json::Value,
    ) -> Self {
        Self {
            name,
            description,
            parameters,
            domain: ToolDomain::Orchestrator,
            always_approve: false,
            eligibility: HostedToolEligibility::Eligible,
            output: StubOutput::EchoParams,
        }
    }
}

#[async_trait]
impl Tool for StubTool {
    fn name(&self) -> &str {
        self.name
    }

    fn description(&self) -> &str {
        self.description
    }

    fn parameters_schema(&self) -> serde_json::Value {
        self.parameters.clone()
    }

    fn domain(&self) -> ToolDomain {
        self.domain
    }

    fn requires_approval(&self, _params: &serde_json::Value) -> ApprovalRequirement {
        if self.always_approve {
            ApprovalRequirement::Always
        } else {
            ApprovalRequirement::Never
        }
    }

    fn hosted_tool_eligibility(&self) -> HostedToolEligibility {
        self.eligibility
    }

    async fn execute(
        &self,
        params: serde_json::Value,
        _ctx: &JobContext,
    ) -> Result<ToolOutput, ToolError> {
        match &self.output {
            StubOutput::EchoParams => Ok(ToolOutput::success(params, Duration::from_millis(5))),
            StubOutput::Fixed(v) => Ok(ToolOutput::success(v.clone(), Duration::from_millis(5))),
            StubOutput::Panic(msg) => panic!("{}", msg),
        }
    }
}

/// Hosted-safe fixture whose approval requirement depends on input params.
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

/// Fixture tool that records the `JobContext.job_id` seen during execution.
pub(crate) struct JobAwareTool {
    /// Shared slot used by tests to observe the executed job id.
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

/// Fixed execution-error modes used to verify HTTP status mapping in tests.
#[derive(Clone, Copy, Debug)]
pub(crate) enum ExecuteErrorKind {
    /// Simulate invalid tool parameters and expect `400 Bad Request`.
    InvalidParameters,
    /// Simulate an authorization failure and expect `403 Forbidden`.
    NotAuthorized,
    /// Simulate rate limiting and expect `429 Too Many Requests`.
    RateLimited,
    /// Simulate a generic execution failure and expect `502 Bad Gateway`.
    ExecutionFailed,
}

/// Fixture tool that returns a chosen [`ExecuteErrorKind`] when executed.
pub(crate) struct ErrorTool {
    /// Hosted tool name exposed through the remote-tool execution route.
    pub(crate) name: &'static str,
    /// Specific execution failure to surface for status-mapping assertions.
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
