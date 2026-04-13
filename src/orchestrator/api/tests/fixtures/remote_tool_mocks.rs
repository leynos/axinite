//! Shared mock tools for hosted remote-tool catalogue endpoint tests.

use std::sync::Arc;
use std::time::Duration;

use tokio::sync::Mutex;
use uuid::Uuid;

use crate::context::JobContext;
use crate::tools::{
    ApprovalRequirement, HostedToolCatalogSource, HostedToolEligibility, NativeTool, Tool,
    ToolDomain, ToolError, ToolOutput,
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
    pub(crate) description: String,
    pub(crate) parameters: serde_json::Value,
    pub(crate) domain: ToolDomain,
    pub(crate) always_approve: bool,
    pub(crate) eligibility: HostedToolEligibility,
    pub(crate) catalog_source: Option<HostedToolCatalogSource>,
    pub(crate) output: StubOutput,
}

impl StubTool {
    pub(crate) fn hosted(
        name: &'static str,
        description: impl Into<String>,
        parameters: serde_json::Value,
    ) -> Self {
        Self {
            name,
            description: description.into(),
            parameters,
            domain: ToolDomain::Orchestrator,
            always_approve: false,
            eligibility: HostedToolEligibility::Eligible,
            catalog_source: Some(HostedToolCatalogSource::Mcp),
            output: StubOutput::EchoParams,
        }
    }

    pub(crate) fn hosted_wasm(
        name: &'static str,
        description: impl Into<String>,
        parameters: serde_json::Value,
    ) -> Self {
        Self {
            catalog_source: Some(HostedToolCatalogSource::Wasm),
            ..Self::hosted(name, description, parameters)
        }
    }
}

impl NativeTool for StubTool {
    fn name(&self) -> &str {
        self.name
    }

    fn description(&self) -> &str {
        &self.description
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

    fn hosted_tool_catalog_source(&self) -> Option<HostedToolCatalogSource> {
        self.catalog_source
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

#[derive(Clone, Copy, Debug)]
/// Shared hosted-remote-tool fixture presets for catalogue and execute tests.
///
/// `CatalogAlpha`, `CatalogAlphaWithDifferentPayload`, `CatalogBeta`, and
/// `CatalogWasm` model hosted-safe catalogue entries. `ApprovalGated` models
/// a hosted tool that must never execute without approval.
pub(crate) enum ToolFixture {
    CatalogAlpha,
    CatalogAlphaWithDifferentPayload,
    CatalogBeta,
    CatalogWasm,
    ApprovalGated,
    ContainerOnly,
}

/// Build an `Arc<dyn Tool>` configured for the requested hosted-tool fixture.
///
/// The returned fixture preserves the canonical names, descriptions, schemas,
/// and panic messages used throughout the remote-tool test suite.
pub(crate) fn build_tool_fixture(kind: ToolFixture) -> Arc<dyn Tool> {
    match kind {
        ToolFixture::CatalogAlpha => Arc::new(StubTool::hosted(
            "remote_tool_catalog_fixture",
            "Hosted-safe tool for catalog tests",
            serde_json::json!({
                "type":"object",
                "properties":{"query":{"type":"string","description":"search query"}},
                "required":["query"]
            }),
        )) as Arc<dyn Tool>,
        ToolFixture::CatalogAlphaWithDifferentPayload => Arc::new(StubTool::hosted(
            "remote_tool_catalog_fixture",
            "Hosted-safe tool for catalog tests with updated payload",
            serde_json::json!({
                "type":"object",
                "properties":{
                    "query":{"type":"string","description":"search query"},
                    "limit":{"type":"integer","minimum":1}
                },
                "required":["query", "limit"]
            }),
        )) as Arc<dyn Tool>,
        ToolFixture::CatalogBeta => Arc::new(StubTool::hosted(
            "remote_tool_catalog_fixture_beta",
            "Second hosted-safe tool for catalog tests",
            serde_json::json!({
                "type":"object",
                "properties":{"path":{"type":"string"}},
                "required":["path"]
            }),
        )) as Arc<dyn Tool>,
        ToolFixture::CatalogWasm => Arc::new(StubTool::hosted_wasm(
            "remote_tool_catalog_fixture_wasm",
            "Hosted-safe WASM tool for catalog tests",
            serde_json::json!({
                "type":"object",
                "properties":{"repository":{"type":"string","description":"repository name"}},
                "required":["repository"]
            }),
        )) as Arc<dyn Tool>,
        ToolFixture::ApprovalGated => Arc::new(StubTool {
            always_approve: true,
            eligibility: HostedToolEligibility::ApprovalGated,
            output: StubOutput::Panic("approval-gated tool must not execute"),
            ..StubTool::hosted(
                "remote_tool_execute_gated",
                "Approval-gated tool",
                serde_json::json!({"type":"object","properties":{}}),
            )
        }) as Arc<dyn Tool>,
        ToolFixture::ContainerOnly => Arc::new(StubTool {
            domain: ToolDomain::Container,
            output: StubOutput::Panic("container-only tool must not execute"),
            ..StubTool::hosted(
                "remote_tool_execute_container",
                "Container-only tool",
                serde_json::json!({"type":"object","properties":{}}),
            )
        }) as Arc<dyn Tool>,
    }
}

/// Hosted-safe fixture whose approval requirement depends on input params.
pub(crate) struct ParamAwareHostedTool;

impl NativeTool for ParamAwareHostedTool {
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

    fn hosted_tool_catalog_source(&self) -> Option<HostedToolCatalogSource> {
        Some(HostedToolCatalogSource::Mcp)
    }
}

/// Fixture tool that records the `JobContext.job_id` seen during execution.
pub(crate) struct JobAwareTool {
    /// Shared slot used by tests to observe the executed job id.
    pub(crate) seen_job_id: Arc<Mutex<Option<Uuid>>>,
}

impl NativeTool for JobAwareTool {
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

    fn hosted_tool_catalog_source(&self) -> Option<HostedToolCatalogSource> {
        Some(HostedToolCatalogSource::Mcp)
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

impl NativeTool for ErrorTool {
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

    fn hosted_tool_catalog_source(&self) -> Option<HostedToolCatalogSource> {
        Some(HostedToolCatalogSource::Mcp)
    }
}

/// Returns a complex `ToolDefinition` with nested JSON Schema structures, a
/// multi-sentence description with special characters and Markdown formatting,
/// and multiple required fields.
///
/// This fixture is used to test that the full `ToolDefinition` payload survives
/// the journey from orchestrator registry through the catalogue endpoint to the
/// worker-side proxy without field loss or transformation.
pub(crate) fn complex_tool_definition() -> crate::llm::ToolDefinition {
    crate::test_support::build_complex_tool_definition(
        "remote_tool_fidelity_fixture",
        concat!(
            "A **complex** tool for testing schema fidelity. ",
            "Handles UTF-8: \u{1F680}\u{1F4A1}. ",
            "Supports `inline code` and [markdown](https://example.com). ",
            "Special chars: <>&\"'{}[]()."
        ),
    )
}

/// Returns a [`StubTool`] configured with the complex tool definition from
/// [`complex_tool_definition`].
///
/// The returned tool is hosted-safe and will echo its params on execution,
/// making it suitable for round-trip fidelity tests.
pub(crate) fn complex_tool_stub() -> StubTool {
    let def = complex_tool_definition();
    StubTool::hosted(
        "remote_tool_fidelity_fixture",
        def.description,
        def.parameters,
    )
}
