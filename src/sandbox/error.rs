//! Error types for the Docker execution sandbox.

use std::time::Duration;

/// Errors that can occur in the sandbox system.
#[derive(Debug, thiserror::Error)]
pub enum SandboxError {
    /// Docker daemon is not available or not running.
    #[error("Docker not available: {reason}")]
    DockerNotAvailable {
        /// Why the daemon could not be reached, as reported by the client.
        reason: String,
    },

    /// Failed to create container.
    #[error("Container creation failed: {reason}")]
    ContainerCreationFailed {
        /// Message from the daemon or the local configuration for the failed create.
        reason: String,
    },

    /// Failed to start container.
    #[error("Container start failed: {reason}")]
    ContainerStartFailed {
        /// Message from the daemon for the failed start.
        reason: String,
    },

    /// Command execution failed inside container.
    #[error("Execution failed: {reason}")]
    ExecutionFailed {
        /// What went wrong while running the command.
        reason: String,
    },

    /// Command timed out.
    #[error("Command timed out after {0:?}")]
    Timeout(Duration),

    /// Container resource limit exceeded.
    #[error("Resource limit exceeded: {resource} limit of {limit}")]
    ResourceLimitExceeded {
        /// Name of the resource whose limit was hit.
        resource: String,
        /// The configured limit that was exceeded, as display text.
        limit: String,
    },

    /// Network proxy error.
    #[error("Proxy error: {reason}")]
    ProxyError {
        /// Failure in the proxy, such as failing to bind its listener.
        reason: String,
    },

    /// Network request blocked by policy.
    #[error("Network request blocked: {reason}")]
    NetworkBlocked {
        /// Why the network policy refused the request.
        reason: String,
    },

    /// Credential injection failed.
    #[error("Credential injection failed for {domain}: {reason}")]
    CredentialInjectionFailed {
        /// Domain the credential was to be injected for.
        domain: String,
        /// Why the injection failed.
        reason: String,
    },

    /// Docker API error.
    #[cfg(feature = "docker")]
    #[error("Docker API error: {0}")]
    Docker(#[from] bollard::errors::Error),

    /// I/O error.
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    /// Configuration error.
    #[error("Configuration error: {reason}")]
    Config {
        /// Why the sandbox configuration was rejected.
        reason: String,
    },
}

/// Result type for sandbox operations.
pub type Result<T> = std::result::Result<T, SandboxError>;
