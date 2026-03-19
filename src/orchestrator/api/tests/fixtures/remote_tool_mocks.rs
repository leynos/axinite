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

enum StubExecute {
    EchoParams,
    Fixed(serde_json::Value),
    Panic(&'static str),
}

pub(crate) struct StubTool {
    name: &'static str,
    description: &'static str,
    parameters: serde_json::Value,
    container_domain: bool,
    always_approve: bool,
    approval_gated: bool,
    execute_behaviour: StubExecute,
}

impl StubTool {
    pub(crate) fn new(name: &'static str) -> Self {
        Self {
            name,
            description: "",
            parameters: serde_json::json!({"type": "object", "properties": {}}),
            container_domain: false,
            always_approve: false,
            approval_gated: false,
            execute_behaviour: StubExecute::EchoParams,
        }
    }

    pub(crate) fn description(mut self, d: &'static str) -> Self {
        self.description = d;
        self
    }

    pub(crate) fn parameters(mut self, p: serde_json::Value) -> Self {
        self.parameters = p;
        self
    }

    pub(crate) fn container_domain(mut self) -> Self {
        self.container_domain = true;
        self
    }

    pub(crate) fn always_approve(mut self) -> Self {
        self.always_approve = true;
        self
    }

    pub(crate) fn approval_gated(mut self) -> Self {
        self.approval_gated = true;
        self
    }

    pub(crate) fn fixed_output(mut self, v: serde_json::Value) -> Self {
        self.execute_behaviour = StubExecute::Fixed(v);
        self
    }

    pub(crate) fn panics_on_execute(mut self, msg: &'static str) -> Self {
        self.execute_behaviour = StubExecute::Panic(msg);
        self
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
        if self.container_domain {
            ToolDomain::Container
        } else {
            ToolDomain::Orchestrator
        }
    }

    fn requires_approval(&self, _params: &serde_json::Value) -> ApprovalRequirement {
        if self.always_approve {
            ApprovalRequirement::Always
        } else {
            ApprovalRequirement::Never
        }
    }

    fn hosted_tool_eligibility(&self) -> HostedToolEligibility {
        if self.approval_gated {
            HostedToolEligibility::ApprovalGated
        } else {
            HostedToolEligibility::Eligible
        }
    }

    async fn execute(
        &self,
        params: serde_json::Value,
        _ctx: &JobContext,
    ) -> Result<ToolOutput, ToolError> {
        match &self.execute_behaviour {
            StubExecute::EchoParams => Ok(ToolOutput::success(params, Duration::from_millis(5))),
            StubExecute::Fixed(v) => Ok(ToolOutput::success(v.clone(), Duration::from_millis(5))),
            StubExecute::Panic(msg) => panic!("{}", msg),
        }
    }
}

pub(crate) fn hosted_catalog_tool() -> StubTool {
    StubTool::new("remote_tool_catalog_fixture")
        .description("Hosted-safe tool for catalog tests")
        .parameters(serde_json::json!({
            "type": "object",
            "properties": {"query": {"type": "string", "description": "search query"}},
            "required": ["query"]
        }))
}

pub(crate) fn hosted_catalog_tool_beta() -> StubTool {
    StubTool::new("remote_tool_catalog_fixture_beta")
        .description("Second hosted-safe tool for catalog tests")
        .parameters(serde_json::json!({
            "type": "object",
            "properties": {"path": {"type": "string"}},
            "required": ["path"]
        }))
}

pub(crate) fn protected_orchestration_tool() -> StubTool {
    StubTool::new("create_job")
        .description("Protected orchestration tool")
        .fixed_output(serde_json::json!({"created": true}))
}

pub(crate) fn protected_job_events_tool() -> StubTool {
    StubTool::new("job_events")
        .description("Protected job-events tool")
        .fixed_output(serde_json::json!({"events": []}))
}

pub(crate) fn approval_gated_tool() -> StubTool {
    StubTool::new("remote_tool_execute_gated")
        .description("Approval-gated tool")
        .always_approve()
        .approval_gated()
        .panics_on_execute("approval-gated tool must not execute")
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

pub(crate) fn container_only_tool() -> StubTool {
    StubTool::new("remote_tool_execute_container")
        .description("Container-only tool")
        .container_domain()
        .panics_on_execute("container-only tool must not execute")
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
