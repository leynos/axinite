//! Orchestrator-side error types.

use uuid::Uuid;

/// Orchestrator errors (internal API, container management).
#[derive(Debug, thiserror::Error)]
pub enum OrchestratorError {
    /// Creating the container for a job failed.
    #[error("Container creation failed for job {job_id}: {reason}")]
    ContainerCreationFailed {
        /// Job the container was being created for.
        job_id: Uuid,
        /// Failure detail from the container creation attempt.
        reason: String,
    },

    /// No container exists for the job.
    #[error("Container not found for job {job_id}")]
    ContainerNotFound {
        /// Job whose container was looked up.
        job_id: Uuid,
    },

    /// The job's container exists but is in a state the requested operation does not accept.
    #[error("Container for job {job_id} is in unexpected state: {state}")]
    InvalidContainerState {
        /// Job whose container was inspected.
        job_id: Uuid,
        /// Container state that was observed.
        state: String,
    },

    /// The orchestrator's internal API returned an error.
    #[error("Internal API error: {reason}")]
    ApiError {
        /// Error detail for the failed API call.
        reason: String,
    },

    /// The Docker daemon returned an error.
    #[error("Docker error: {reason}")]
    Docker {
        /// Error message reported by Docker.
        reason: String,
    },
}
