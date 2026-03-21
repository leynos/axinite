//! Tool trait and types.

mod approval_policy;
mod schema_helpers;
mod traits;

pub use approval_policy::{
    ApprovalContext, ApprovalRequirement, HostedToolEligibility, ToolDomain, ToolRateLimitConfig,
};
pub use schema_helpers::{redact_params, require_param, require_str, validate_tool_schema};
pub use traits::{Tool, ToolError, ToolOutput};

#[cfg(test)]
mod tests;
