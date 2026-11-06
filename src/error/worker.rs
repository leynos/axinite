//! Worker-side error types.

use std::time::Duration;

use uuid::Uuid;

/// Configuration field that mismatched between worker config and HTTP client.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConfigMismatchField {
    /// The job_id field mismatched.
    JobId,
    /// The orchestrator_url field mismatched.
    OrchestratorUrl,
}

impl std::fmt::Display for ConfigMismatchField {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::JobId => write!(f, "job_id"),
            Self::OrchestratorUrl => write!(f, "orchestrator_url"),
        }
    }
}

/// Worker errors (container-side execution).
#[derive(Debug, thiserror::Error)]
pub enum WorkerError {
    /// The worker could not reach the orchestrator endpoint at `url`.
    ///
    /// `reason` contains the lower-level transport or HTTP-client failure.
    #[error("Failed to connect to orchestrator at {url}: {reason}")]
    ConnectionFailed {
        /// Orchestrator endpoint the worker tried to reach.
        url: String,
        /// Transport or HTTP-client failure text.
        reason: String,
    },

    /// The worker failed while proxying an LLM request through the orchestrator.
    ///
    /// `reason` contains the upstream request or response failure detail.
    #[error("LLM proxy request failed: {reason}")]
    LlmProxyFailed {
        /// Failure detail from the proxied upstream request or response.
        reason: String,
    },

    /// The orchestrator rejected the worker's request as malformed.
    ///
    /// `reason` describes the validation or request-shape failure.
    #[error("Bad request from orchestrator: {reason}")]
    BadRequest {
        /// Validation or request-shape failure text.
        reason: String,
    },

    /// The orchestrator rejected the worker's request for authorization reasons.
    ///
    /// `reason` contains the rejection detail returned by the orchestrator.
    #[error("Unauthorized remote tool request: {reason}")]
    Unauthorized {
        /// Rejection detail returned by the orchestrator.
        reason: String,
    },

    /// The orchestrator or upstream service rate-limited the worker request.
    ///
    /// `reason` contains the returned failure detail. `retry_after`, when
    /// present, is the parsed `Retry-After` delay supplied by the response.
    #[error("Remote tool request was rate limited: {reason}")]
    RateLimited {
        /// Failure detail returned with the rate-limit response.
        reason: String,
        /// Delay parsed from the `Retry-After` header, or `None` if absent.
        retry_after: Option<Duration>,
    },

    /// The orchestrator reported an upstream gateway failure for the request.
    ///
    /// `reason` contains the upstream error detail.
    #[error("Remote tool request failed upstream: {reason}")]
    BadGateway {
        /// Upstream error detail reported by the orchestrator.
        reason: String,
    },

    /// The remote-tool request failed for a non-specialized upstream reason.
    ///
    /// `reason` contains the orchestrator status and response body summary.
    #[error("Remote tool request failed: {reason}")]
    RemoteToolFailed {
        /// Orchestrator status and response body summary.
        reason: String,
    },

    /// Secret resolution failed for the named secret before worker execution.
    ///
    /// `secret_name` identifies the requested secret, and `reason` describes
    /// why it could not be resolved.
    #[error("Secret resolution failed for {secret_name}: {reason}")]
    SecretResolveFailed {
        /// Name of the secret that could not be resolved.
        secret_name: String,
        /// Why the secret could not be resolved.
        reason: String,
    },

    /// The orchestrator rejected work for the specified job id.
    ///
    /// `job_id` identifies the affected worker job, and `reason` contains the
    /// orchestrator's response detail.
    #[error("Orchestrator returned error for job {job_id}: {reason}")]
    OrchestratorRejected {
        /// Job the orchestrator rejected work for.
        job_id: Uuid,
        /// Response detail from the orchestrator.
        reason: String,
    },

    /// The worker failed while performing local execution steps.
    ///
    /// `reason` contains the execution failure detail.
    #[error("Worker execution failed: {reason}")]
    ExecutionFailed {
        /// Failure detail from the local execution step.
        reason: String,
    },

    /// The worker token environment variable was not available at startup.
    #[error("Missing worker token (AXINITE_WORKER_TOKEN not set)")]
    MissingToken,

    /// The worker configuration does not match the provided HTTP client.
    ///
    /// `field` identifies which configuration field mismatched, and `reason`
    /// describes the mismatch.
    #[error("Worker configuration mismatch for {field}: {reason}")]
    ConfigMismatch {
        /// Configuration field that disagreed with the HTTP client.
        field: ConfigMismatchField,
        /// Description of the disagreement.
        reason: String,
    },
}
