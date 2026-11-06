//! Tool execution error types.

use std::time::Duration;

/// Tool execution errors.
#[derive(Debug, thiserror::Error)]
pub enum ToolError {
    /// No tool is registered under the requested name.
    #[error("Tool {name} not found")]
    NotFound {
        /// Requested tool name.
        name: String,
    },

    /// The tool ran and returned an error. `reason` carries the error text.
    #[error("Tool {name} execution failed: {reason}")]
    ExecutionFailed {
        /// Name of the tool that failed.
        name: String,
        /// Error text from the tool's execution.
        reason: String,
    },

    /// The tool did not finish within its time limit.
    #[error("Tool {name} timed out after {timeout:?}")]
    Timeout {
        /// Name of the tool that timed out.
        name: String,
        /// The limit that was exceeded.
        timeout: Duration,
    },

    /// The arguments supplied to the tool were rejected.
    #[error("Invalid parameters for tool {name}: {reason}")]
    InvalidParameters {
        /// Name of the tool that rejected its arguments.
        name: String,
        /// Why the arguments were rejected.
        reason: String,
    },

    /// The tool is switched off. `reason` says why.
    #[error("Tool {name} is disabled: {reason}")]
    Disabled {
        /// Name of the disabled tool.
        name: String,
        /// Why the tool is disabled.
        reason: String,
    },

    /// The sandbox failed or refused the tool's call.
    #[error("Sandbox error for tool {name}: {reason}")]
    Sandbox {
        /// Name of the tool whose call was sandboxed.
        name: String,
        /// Sandbox error text.
        reason: String,
    },

    /// The tool needs credentials that have not been supplied.
    #[error("Tool {name} requires authentication")]
    AuthRequired {
        /// Name of the tool that needs authentication.
        name: String,
    },

    /// The tool's upstream service refused the call because of rate limits.
    #[error("Tool {name} is rate limited, retry after {retry_after:?}")]
    RateLimited {
        /// Name of the rate-limited tool.
        name: String,
        /// How long to wait before retrying, when the source supplied one.
        retry_after: Option<Duration>,
    },

    /// Building the tool failed. Carries the builder's error message.
    #[error("Tool builder failed: {0}")]
    BuilderFailed(String),
}
