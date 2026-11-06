//! Error types for WASM channels.

use std::path::PathBuf;

/// Error during WASM channel operations.
#[derive(Debug, thiserror::Error)]
pub enum WasmChannelError {
    /// The channel failed to start; `reason` carries the underlying error.
    #[error("Channel {name} failed to start: {reason}")]
    StartupFailed {
        /// Name of the channel that failed to start.
        name: String,
        /// Underlying error message from the failed start.
        reason: String,
    },

    /// A channel callback returned an error; `reason` carries its message.
    #[error("Channel {name} callback failed: {reason}")]
    CallbackFailed {
        /// Name of the channel whose callback failed.
        name: String,
        /// Error message returned by the callback.
        reason: String,
    },

    /// The WASM guest trapped during execution, for example on an unreachable instruction.
    #[error("Channel {name} WASM execution trapped: {reason}")]
    Trapped {
        /// Name of the channel that trapped.
        name: String,
        /// Description of the trap.
        reason: String,
    },

    /// A guest callback did not return within its time limit.
    #[error("Channel {name} callback '{callback}' timed out")]
    Timeout {
        /// Name of the channel whose callback timed out.
        name: String,
        /// Guest callback that timed out, such as `on_start`.
        callback: String,
    },

    /// Host-side execution of the channel panicked; `reason` describes the panic.
    #[error("Channel {name} execution panicked: {reason}")]
    ExecutionPanicked {
        /// Name of the channel whose execution panicked.
        name: String,
        /// Description of the panic.
        reason: String,
    },

    /// The channel emitted messages faster than its rate limit allows.
    #[error("Channel {name} emit rate limited")]
    EmitRateLimited {
        /// Name of the rate-limited channel.
        name: String,
    },

    /// A request targeted an HTTP path the channel is not permitted to serve.
    #[error("Channel {name} HTTP path not allowed: {path}")]
    PathNotAllowed {
        /// Name of the channel that received the request.
        name: String,
        /// The rejected request path.
        path: String,
    },

    /// A requested polling interval was below the permitted minimum.
    #[error("Channel {name} polling interval too short: {interval_ms}ms (minimum: {min_ms}ms)")]
    PollIntervalTooShort {
        /// Name of the channel that requested the interval.
        name: String,
        /// Requested polling interval, in milliseconds.
        interval_ms: u32,
        /// Smallest permitted polling interval, in milliseconds.
        min_ms: u32,
    },

    /// A file path resolved outside the channel's workspace.
    #[error("Channel {name} workspace path escape attempt: {path}")]
    WorkspaceEscape {
        /// Name of the channel that attempted the access.
        name: String,
        /// The offending path.
        path: String,
    },

    /// The channel ran out of its WASM fuel budget.
    #[error("Channel {name} exhausted fuel limit ({limit})")]
    FuelExhausted {
        /// Name of the channel that exhausted its fuel.
        name: String,
        /// Fuel budget the channel was allotted.
        limit: u64,
    },

    /// An I/O operation failed.
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    /// The WASM component file does not exist at the given path.
    #[error("WASM file not found: {0}")]
    WasmNotFound(PathBuf),

    /// The capabilities file does not exist at the given path.
    #[error("Capabilities file not found: {0}")]
    CapabilitiesNotFound(PathBuf),

    /// The capabilities document could not be parsed; the string carries the parser message.
    #[error("Invalid capabilities JSON: {0}")]
    InvalidCapabilities(String),

    /// The WASM component failed to compile; the string carries the compiler message.
    #[error("WASM compilation error: {0}")]
    Compilation(String),

    /// The compiled component could not be instantiated; the string carries the reason.
    #[error("WASM instantiation error: {0}")]
    Instantiation(String),

    /// The channel name failed validation; the string gives the rejected name or reason.
    #[error("Invalid channel name: {0}")]
    InvalidName(String),

    /// No channel with the requested name is registered.
    #[error("Channel {name} not found")]
    NotFound {
        /// Name that was looked up.
        name: String,
    },

    /// The channel module does not export a function the host requires; the string names it.
    #[error("Channel module missing export: {0}")]
    MissingExport(String),

    /// The guest returned a response the host could not interpret.
    #[error("Invalid response from WASM: {0}")]
    InvalidResponse(String),

    /// The WASM runtime was used before it was initialized.
    #[error("Runtime not initialized")]
    RuntimeNotInitialized,

    /// The channel configuration is invalid; the string describes the problem.
    #[error("Configuration error: {0}")]
    Config(String),

    /// Registering the channel's webhook failed.
    #[error("Webhook registration failed for channel {name}: {reason}")]
    WebhookRegistration {
        /// Name of the channel whose webhook failed to register.
        name: String,
        /// Underlying error message.
        reason: String,
    },

    /// An outbound HTTP request made on the channel's behalf failed.
    #[error("HTTP request error: {0}")]
    HttpRequest(String),

    /// The component targets a WIT interface version the host does not support.
    #[error("WIT version mismatch: {0}")]
    IncompatibleWitVersion(String),
}

impl From<crate::tools::wasm::WasmError> for WasmChannelError {
    fn from(err: crate::tools::wasm::WasmError) -> Self {
        WasmChannelError::Compilation(err.to_string())
    }
}
