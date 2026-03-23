use core::future::Future;
use core::pin::Pin;
use std::time::Duration;

use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::context::JobContext;

use super::approval_policy::{
    ApprovalRequirement, HostedToolCatalogSource, HostedToolEligibility, ToolDomain,
    ToolRateLimitConfig,
};

/// Boxed future used at the dyn `Tool` boundary.
pub type ToolFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

/// Error type for tool execution.
#[derive(Debug, Error)]
pub enum ToolError {
    #[error("Invalid parameters: {0}")]
    InvalidParameters(String),

    #[error("Execution failed: {0}")]
    ExecutionFailed(String),

    #[error("Timeout after {0:?}")]
    Timeout(Duration),

    #[error("Not authorized: {0}")]
    NotAuthorized(String),

    #[error("Rate limited, retry after {0:?}")]
    RateLimited(Option<Duration>),

    #[error("External service error: {0}")]
    ExternalService(String),

    #[error("Sandbox error: {0}")]
    Sandbox(String),
}

/// Output from a tool execution.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolOutput {
    /// The result data.
    pub result: serde_json::Value,
    /// Cost incurred (if any).
    pub cost: Option<Decimal>,
    /// Time taken.
    pub duration: Duration,
    /// Raw output before sanitization (for debugging).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub raw: Option<String>,
}

impl ToolOutput {
    /// Create a successful output with a JSON result.
    pub fn success(result: serde_json::Value, duration: Duration) -> Self {
        Self {
            result,
            cost: None,
            duration,
            raw: None,
        }
    }

    /// Create a text output.
    pub fn text(text: impl Into<String>, duration: Duration) -> Self {
        Self {
            result: serde_json::Value::String(text.into()),
            cost: None,
            duration,
            raw: None,
        }
    }

    /// Set the cost.
    pub fn with_cost(mut self, cost: Decimal) -> Self {
        self.cost = Some(cost);
        self
    }

    /// Set the raw output.
    pub fn with_raw(mut self, raw: impl Into<String>) -> Self {
        self.raw = Some(raw.into());
        self
    }
}

/// Definition of a tool's parameters using JSON Schema.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolSchema {
    pub name: String,
    pub description: String,
    pub parameters: serde_json::Value,
}

impl ToolSchema {
    /// Create a new tool schema.
    pub fn new(name: impl Into<String>, description: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            description: description.into(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {},
                "required": []
            }),
        }
    }

    /// Set the parameters schema.
    pub fn with_parameters(mut self, parameters: serde_json::Value) -> Self {
        self.parameters = parameters;
        self
    }
}

/// Trait for tools that the agent can use.
///
/// This is the dyn-safe object boundary. Concrete implementations should
/// implement [`NativeTool`] instead; the blanket adapter provides this
/// trait automatically.
pub trait Tool: Send + Sync {
    /// Get the tool name.
    fn name(&self) -> &str;

    /// Get a description of what the tool does.
    fn description(&self) -> &str;

    /// Get the JSON Schema for the tool's parameters.
    fn parameters_schema(&self) -> serde_json::Value;

    /// Execute the tool with the given parameters.
    fn execute<'a>(
        &'a self,
        params: serde_json::Value,
        ctx: &'a JobContext,
    ) -> ToolFuture<'a, Result<ToolOutput, ToolError>>;

    /// Estimate the cost of running this tool with the given parameters.
    fn estimated_cost(&self, _params: &serde_json::Value) -> Option<Decimal> {
        None
    }

    /// Estimate how long this tool will take with the given parameters.
    fn estimated_duration(&self, _params: &serde_json::Value) -> Option<Duration> {
        None
    }

    /// Whether this tool's output needs sanitization.
    ///
    /// Returns true for tools that interact with external services,
    /// where the output might contain malicious content.
    fn requires_sanitization(&self) -> bool {
        true
    }

    /// Whether this tool invocation requires user approval.
    ///
    /// Returns `Never` by default (most tools run in a sandboxed environment).
    /// Override to return `UnlessAutoApproved` for tools that need approval
    /// but can be session-auto-approved, or `Always` for invocations that
    /// must always prompt (e.g. destructive shell commands, HTTP with auth).
    fn requires_approval(&self, _params: &serde_json::Value) -> ApprovalRequirement {
        ApprovalRequirement::Never
    }

    /// Whether hosted workers may advertise this tool in the remote catalog.
    ///
    /// This must not infer visibility from placeholder params because some
    /// tools decide approval based on real invocation data. Override this when
    /// hosted visibility needs an explicit policy.
    fn hosted_tool_eligibility(&self) -> HostedToolEligibility {
        HostedToolEligibility::Eligible
    }

    /// Which hosted-catalogue source family this tool belongs to, if any.
    ///
    /// Most built-in tools should return `None`, because being safe to execute
    /// is not enough to make them part of the hosted-visible remote catalogue.
    /// Dynamic tool wrappers such as MCP and later WASM adapters should opt in
    /// explicitly so the canonical registry filter can project only the tool
    /// families the current roadmap step is ready to advertise.
    fn hosted_tool_catalog_source(&self) -> Option<HostedToolCatalogSource> {
        None
    }

    /// Maximum time this tool is allowed to run before the caller kills it.
    /// Override for long-running tools like sandbox execution.
    /// Default: 60 seconds.
    fn execution_timeout(&self) -> Duration {
        Duration::from_secs(60)
    }

    /// Where this tool should execute.
    ///
    /// `Orchestrator` tools run in the main agent process (safe, no FS access).
    /// `Container` tools run inside Docker containers (shell, file ops).
    ///
    /// Default: `Orchestrator` (safe for the main process).
    fn domain(&self) -> ToolDomain {
        ToolDomain::Orchestrator
    }

    /// Parameter names whose values must be redacted before logging, hooks, and approvals.
    ///
    /// The agent framework replaces these parameter values with `"[REDACTED]"` before:
    /// - Writing to debug logs
    /// - Storing in `ActionRecord` (in-memory job history)
    /// - Recording in `TurnToolCall` (session state)
    /// - Sending to `BeforeToolCall` hooks
    /// - Displaying in the approval UI
    ///
    /// **The `execute()` method still receives the original, unredacted parameters.**
    /// Redaction only applies to the observability and audit paths, not execution.
    ///
    /// Use this for tools that accept plaintext secrets as parameters (e.g. `secret_save`).
    fn sensitive_params(&self) -> &[&str] {
        &[]
    }

    /// Per-invocation rate limit for this tool.
    ///
    /// Return `Some(config)` to throttle how often this tool can be called per user.
    /// Read-only tools (echo, time, json, file_read, memory_search, etc.) should
    /// return `None`. Write/external tools (shell, http, file_write, memory_write,
    /// create_job) should return sensible limits to prevent runaway agents.
    ///
    /// Rate limits are per-user, per-tool, and in-memory (reset on restart).
    /// This is orthogonal to `requires_approval()` — a tool can be both
    /// approval-gated and rate limited. Rate limit is checked first (cheaper).
    ///
    /// Default: `None` (no rate limiting).
    fn rate_limit_config(&self) -> Option<ToolRateLimitConfig> {
        None
    }

    /// Get the tool schema for LLM function calling.
    fn schema(&self) -> ToolSchema {
        ToolSchema {
            name: self.name().to_string(),
            description: self.description().to_string(),
            parameters: self.parameters_schema(),
        }
    }
}

/// Native (non-dyn) sibling of [`Tool`] for concrete implementations.
///
/// Implement this trait instead of [`Tool`] directly. The blanket adapter
/// below automatically implements [`Tool`] for every `T: NativeTool`.
pub trait NativeTool: Send + Sync {
    /// Get the tool name.
    fn name(&self) -> &str;

    /// Get a description of what the tool does.
    fn description(&self) -> &str;

    /// Get the JSON Schema for the tool's parameters.
    fn parameters_schema(&self) -> serde_json::Value;

    /// Execute the tool with the given parameters.
    fn execute<'a>(
        &'a self,
        params: serde_json::Value,
        ctx: &'a JobContext,
    ) -> impl Future<Output = Result<ToolOutput, ToolError>> + Send + 'a;

    /// Estimate the cost of running this tool with the given parameters.
    fn estimated_cost(&self, _params: &serde_json::Value) -> Option<Decimal> {
        None
    }

    /// Estimate how long this tool will take with the given parameters.
    fn estimated_duration(&self, _params: &serde_json::Value) -> Option<Duration> {
        None
    }

    /// Whether this tool's output needs sanitization.
    fn requires_sanitization(&self) -> bool {
        true
    }

    /// Whether this tool invocation requires user approval.
    fn requires_approval(&self, _params: &serde_json::Value) -> ApprovalRequirement {
        ApprovalRequirement::Never
    }

    /// Whether hosted workers may advertise this tool in the remote catalog.
    fn hosted_tool_eligibility(&self) -> HostedToolEligibility {
        HostedToolEligibility::Eligible
    }

    /// Which hosted-catalogue source family this tool belongs to, if any.
    fn hosted_tool_catalog_source(&self) -> Option<HostedToolCatalogSource> {
        None
    }

    /// Maximum time this tool is allowed to run before the caller kills it.
    fn execution_timeout(&self) -> Duration {
        Duration::from_secs(60)
    }

    /// Where this tool should execute.
    fn domain(&self) -> ToolDomain {
        ToolDomain::Orchestrator
    }

    /// Parameter names whose values must be redacted before logging, hooks,
    /// and approvals.
    fn sensitive_params(&self) -> &[&str] {
        &[]
    }

    /// Per-invocation rate limit for this tool.
    fn rate_limit_config(&self) -> Option<ToolRateLimitConfig> {
        None
    }

    /// Get the tool schema for LLM function calling.
    fn schema(&self) -> ToolSchema {
        ToolSchema {
            name: self.name().to_string(),
            description: self.description().to_string(),
            parameters: self.parameters_schema(),
        }
    }
}

impl<T: NativeTool> Tool for T {
    fn name(&self) -> &str {
        NativeTool::name(self)
    }

    fn description(&self) -> &str {
        NativeTool::description(self)
    }

    fn parameters_schema(&self) -> serde_json::Value {
        NativeTool::parameters_schema(self)
    }

    fn execute<'a>(
        &'a self,
        params: serde_json::Value,
        ctx: &'a JobContext,
    ) -> ToolFuture<'a, Result<ToolOutput, ToolError>> {
        Box::pin(NativeTool::execute(self, params, ctx))
    }

    fn estimated_cost(&self, params: &serde_json::Value) -> Option<Decimal> {
        NativeTool::estimated_cost(self, params)
    }

    fn estimated_duration(&self, params: &serde_json::Value) -> Option<Duration> {
        NativeTool::estimated_duration(self, params)
    }

    fn requires_sanitization(&self) -> bool {
        NativeTool::requires_sanitization(self)
    }

    fn requires_approval(&self, params: &serde_json::Value) -> ApprovalRequirement {
        NativeTool::requires_approval(self, params)
    }

    fn hosted_tool_eligibility(&self) -> HostedToolEligibility {
        NativeTool::hosted_tool_eligibility(self)
    }

    fn hosted_tool_catalog_source(&self) -> Option<HostedToolCatalogSource> {
        NativeTool::hosted_tool_catalog_source(self)
    }

    fn execution_timeout(&self) -> Duration {
        NativeTool::execution_timeout(self)
    }

    fn domain(&self) -> ToolDomain {
        NativeTool::domain(self)
    }

    fn sensitive_params(&self) -> &[&str] {
        NativeTool::sensitive_params(self)
    }

    fn rate_limit_config(&self) -> Option<ToolRateLimitConfig> {
        NativeTool::rate_limit_config(self)
    }

    fn schema(&self) -> ToolSchema {
        NativeTool::schema(self)
    }
}
