//! Orchestrator-side error types.

use uuid::Uuid;

/// Orchestrator errors (internal API, container management).
#[derive(Debug, thiserror::Error)]
pub enum OrchestratorError {
    #[error("Container creation failed for job {job_id}: {reason}")]
    ContainerCreationFailed { job_id: Uuid, reason: String },

    #[error("Container not found for job {job_id}")]
    ContainerNotFound { job_id: Uuid },

    #[error("Container for job {job_id} is in unexpected state: {state}")]
    InvalidContainerState { job_id: Uuid, state: String },

    #[error("Internal API error: {reason}")]
    ApiError { reason: String },

    #[error("Docker error: {reason}")]
    Docker { reason: String },
}
