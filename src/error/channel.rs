//! Channel-related error types.

/// Channel-related errors.
#[derive(Debug, thiserror::Error)]
pub enum ChannelError {
    /// The channel could not start. `reason` carries the underlying error.
    #[error("Channel {name} failed to start: {reason}")]
    StartupFailed {
        /// Configured name of the channel.
        name: String,
        /// Underlying startup error text.
        reason: String,
    },

    /// The channel lost its connection and stopped receiving messages.
    #[error("Channel {name} disconnected: {reason}")]
    Disconnected {
        /// Configured name of the channel.
        name: String,
        /// Why the connection was lost.
        reason: String,
    },

    /// A response could not be delivered on the channel.
    #[error("Failed to send response on channel {name}: {reason}")]
    SendFailed {
        /// Configured name of the channel.
        name: String,
        /// Underlying delivery error text.
        reason: String,
    },

    /// An inbound message did not match the format the channel expects. Carries a description
    /// of the defect.
    #[error("Invalid message format: {0}")]
    InvalidMessage(String),

    /// Credentials were rejected or could not be obtained for the channel.
    #[error("Authentication failed for channel {name}: {reason}")]
    AuthFailed {
        /// Configured name of the channel.
        name: String,
        /// Reason reported by the authentication step.
        reason: String,
    },

    /// The channel's upstream service refused the call because of rate limits.
    #[error("Rate limited on channel {name}")]
    RateLimited {
        /// Configured name of the channel that hit the limit.
        name: String,
    },

    /// An HTTP request made on behalf of the channel failed. Carries the transport error text.
    #[error("HTTP error: {0}")]
    Http(String),

    /// A health probe reported the channel as unhealthy.
    #[error("Channel health check failed: {name}")]
    HealthCheckFailed {
        /// Configured name of the channel that failed its health check.
        name: String,
    },
}
