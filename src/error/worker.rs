//! Worker-side error types.

use std::time::Duration;

use uuid::Uuid;

/// Worker errors (container-side execution).
#[derive(Debug, thiserror::Error)]
pub enum WorkerError {
    #[error("Failed to connect to orchestrator at {url}: {reason}")]
    ConnectionFailed { url: String, reason: String },

    #[error("LLM proxy request failed: {reason}")]
    LlmProxyFailed { reason: String },

    #[error("Bad request from orchestrator: {reason}")]
    BadRequest { reason: String },

    #[error("Unauthorized remote tool request: {reason}")]
    Unauthorized { reason: String },

    #[error("Remote tool request was rate limited: {reason}")]
    RateLimited {
        reason: String,
        retry_after: Option<Duration>,
    },

    #[error("Remote tool request failed upstream: {reason}")]
    BadGateway { reason: String },

    #[error("Remote tool request failed: {reason}")]
    RemoteToolFailed { reason: String },

    #[error("Secret resolution failed for {secret_name}: {reason}")]
    SecretResolveFailed { secret_name: String, reason: String },

    #[error("Orchestrator returned error for job {job_id}: {reason}")]
    OrchestratorRejected { job_id: Uuid, reason: String },

    #[error("Worker execution failed: {reason}")]
    ExecutionFailed { reason: String },

    #[error("Missing worker token (IRONCLAW_WORKER_TOKEN not set)")]
    MissingToken,
}
