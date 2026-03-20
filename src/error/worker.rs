//! Worker-side error types.

use std::time::Duration;

use uuid::Uuid;

/// Worker errors (container-side execution).
#[derive(Debug, thiserror::Error)]
pub enum WorkerError {
    /// The worker could not reach the orchestrator endpoint at `url`.
    ///
    /// `reason` contains the lower-level transport or HTTP-client failure.
    #[error("Failed to connect to orchestrator at {url}: {reason}")]
    ConnectionFailed { url: String, reason: String },

    /// The worker failed while proxying an LLM request through the orchestrator.
    ///
    /// `reason` contains the upstream request or response failure detail.
    #[error("LLM proxy request failed: {reason}")]
    LlmProxyFailed { reason: String },

    /// The orchestrator rejected the worker's request as malformed.
    ///
    /// `reason` describes the validation or request-shape failure.
    #[error("Bad request from orchestrator: {reason}")]
    BadRequest { reason: String },

    /// The orchestrator rejected the worker's request for authorization reasons.
    ///
    /// `reason` contains the rejection detail returned by the orchestrator.
    #[error("Unauthorized remote tool request: {reason}")]
    Unauthorized { reason: String },

    /// The orchestrator or upstream service rate-limited the worker request.
    ///
    /// `reason` contains the returned failure detail. `retry_after`, when
    /// present, is the parsed `Retry-After` delay supplied by the response.
    #[error("Remote tool request was rate limited: {reason}")]
    RateLimited {
        reason: String,
        retry_after: Option<Duration>,
    },

    /// The orchestrator reported an upstream gateway failure for the request.
    ///
    /// `reason` contains the upstream error detail.
    #[error("Remote tool request failed upstream: {reason}")]
    BadGateway { reason: String },

    /// The remote-tool request failed for a non-specialised upstream reason.
    ///
    /// `reason` contains the orchestrator status and response body summary.
    #[error("Remote tool request failed: {reason}")]
    RemoteToolFailed { reason: String },

    /// Secret resolution failed for the named secret before worker execution.
    ///
    /// `secret_name` identifies the requested secret, and `reason` describes
    /// why it could not be resolved.
    #[error("Secret resolution failed for {secret_name}: {reason}")]
    SecretResolveFailed { secret_name: String, reason: String },

    /// The orchestrator rejected work for the specified job id.
    ///
    /// `job_id` identifies the affected worker job, and `reason` contains the
    /// orchestrator's response detail.
    #[error("Orchestrator returned error for job {job_id}: {reason}")]
    OrchestratorRejected { job_id: Uuid, reason: String },

    /// The worker failed while performing local execution steps.
    ///
    /// `reason` contains the execution failure detail.
    #[error("Worker execution failed: {reason}")]
    ExecutionFailed { reason: String },

    /// The worker token environment variable was not available at startup.
    #[error("Missing worker token (IRONCLAW_WORKER_TOKEN not set)")]
    MissingToken,
}
