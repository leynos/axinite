//! LLM provider error types.

use std::time::Duration;

/// LLM provider errors.
#[derive(Debug, thiserror::Error)]
pub enum LlmError {
    /// A provider call failed.
    #[error("Provider {provider} request failed: {reason}")]
    RequestFailed {
        /// Name of the provider that was called.
        provider: String,
        /// Description of the failure.
        reason: String,
    },

    /// The provider throttled the request.
    #[error("Provider {provider} rate limited, retry after {retry_after:?}")]
    RateLimited {
        /// Name of the provider that throttled the request.
        provider: String,
        /// Delay the provider asked for before retrying. `None` when it gave no hint.
        retry_after: Option<Duration>,
    },

    /// The provider replied, but the reply could not be understood.
    #[error("Invalid response from {provider}: {reason}")]
    InvalidResponse {
        /// Name of the provider that sent the reply.
        provider: String,
        /// Description of what was wrong with the reply.
        reason: String,
    },

    /// The request needs more tokens than the model's context window allows.
    #[error("Context length exceeded: {used} tokens used, {limit} allowed")]
    ContextLengthExceeded {
        /// Tokens the request needed.
        used: usize,
        /// Maximum tokens the model accepts.
        limit: usize,
    },

    /// The requested model is not offered by the provider.
    #[error("Model {model} not available on provider {provider}")]
    ModelNotAvailable {
        /// Name of the provider that was asked for the model.
        provider: String,
        /// Identifier of the unavailable model.
        model: String,
    },

    /// The provider rejected the credentials.
    #[error("Authentication failed for provider {provider}")]
    AuthFailed {
        /// Name of the provider that rejected the credentials.
        provider: String,
    },

    /// The provider session has expired and must be renewed.
    #[error("Session expired for provider {provider}")]
    SessionExpired {
        /// Name of the provider whose session expired.
        provider: String,
    },

    /// Renewing an expired provider session failed.
    #[error("Session renewal failed for provider {provider}: {reason}")]
    SessionRenewalFailed {
        /// Name of the provider whose session could not be renewed.
        provider: String,
        /// Description of the renewal failure.
        reason: String,
    },

    /// A transport-level HTTP error from `reqwest`.
    #[error("HTTP error: {0}")]
    Http(#[from] reqwest::Error),

    /// A JSON serialization or deserialization error from `serde_json`.
    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),

    /// An I/O error.
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
}
