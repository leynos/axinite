//! Tool trait, native-tool integration types, and parameter helpers.
//!
//! This module defines the core abstractions for tool execution within the
//! agent framework:
//!
//! * [`Tool`] / [`NativeTool`] — traits implemented by every tool, covering
//!   schema declaration, execution, approval requirements, and sanitisation.
//! * [`ToolError`] / [`ToolOutput`] — the standard result types returned by
//!   tool execution.
//! * [`ToolDomain`] / [`ToolRateLimitConfig`] — metadata used by the
//!   registry for routing and throttling.
//! * [`require_str`] / [`require_param`] — typed parameter extraction helpers
//!   that produce consistent [`ToolError::InvalidParameters`] messages.
//! * [`redact_params`] — replaces sensitive parameter values before logging
//!   or approval display.
//! * [`validate_tool_schema`] — lenient runtime validation of a tool's
//!   `parameters_schema()` return value.
//! * [`ParamName`] / [`SchemaPath`] / [`ToolName`] — borrowed newtypes that
//!   make helper-function call sites explicit and type-checked.

mod approval_policy;
mod schema_helpers;
mod traits;

pub use approval_policy::{
    ApprovalContext, ApprovalRequirement, HostedToolCatalogSource, HostedToolEligibility,
    ToolDomain, ToolRateLimitConfig,
};
pub use schema_helpers::{
    ParamName, SchemaPath, ToolName, redact_params, require_param, require_str,
    validate_tool_schema,
};
pub use traits::{NativeTool, Tool, ToolError, ToolFuture, ToolOutput};

#[cfg(test)]
mod tests;
