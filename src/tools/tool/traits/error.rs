//! The error type returned by tool execution.

use std::time::Duration;

use thiserror::Error;

/// Error type for tool execution.
#[derive(Debug, Error)]
pub enum ToolError {
    /// Input failed validation; the tool was not run.
    #[error("Invalid parameters: {0}")]
    InvalidParameters(String),

    /// The tool ran and failed; the string describes the cause.
    #[error("Execution failed: {0}")]
    ExecutionFailed(String),

    /// The tool exceeded its execution timeout; carries the limit that was hit.
    #[error("Timeout after {0:?}")]
    Timeout(Duration),

    /// The caller is not permitted to perform the requested action.
    #[error("Not authorized: {0}")]
    NotAuthorized(String),

    /// A rate limit was hit; carries the suggested wait before retrying, when known.
    #[error("Rate limited, retry after {0:?}")]
    RateLimited(Option<Duration>),

    /// A remote dependency, such as an API or MCP server, returned an error.
    #[error("External service error: {0}")]
    ExternalService(String),

    /// The sandbox that runs the tool, such as a container, reported an error.
    #[error("Sandbox error: {0}")]
    Sandbox(String),
}
