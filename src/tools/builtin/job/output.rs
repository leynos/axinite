//! Shared `ToolOutput` construction helpers for the job tools.
//!
//! The job tools report recoverable failures as successful outputs carrying
//! an `error` payload so the LLM can relay the problem to the user instead
//! of aborting the conversation turn.

use crate::tools::tool::{ToolError, ToolOutput};

/// Wrap a JSON payload as a successful `ToolOutput`, timed from `start`.
pub(super) fn success_output(
    result: serde_json::Value,
    start: std::time::Instant,
) -> Result<ToolOutput, ToolError> {
    Ok(ToolOutput::success(result, start.elapsed()))
}

/// Report an error message as a successful `ToolOutput` carrying an
/// `error` payload, timed from `start`.
pub(super) fn error_output(
    message: String,
    start: std::time::Instant,
) -> Result<ToolOutput, ToolError> {
    success_output(serde_json::json!({ "error": message }), start)
}
