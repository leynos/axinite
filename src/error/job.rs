//! Job-related error types.

use std::time::Duration;

use uuid::Uuid;

/// Job-related errors.
#[derive(Debug, thiserror::Error)]
pub enum JobError {
    #[error("Job {id} not found")]
    NotFound { id: Uuid },

    #[error("Job {id} already in state {state}, cannot transition to {target}")]
    InvalidTransition {
        id: Uuid,
        state: String,
        target: String,
    },

    #[error("Job {id} failed: {reason}")]
    Failed { id: Uuid, reason: String },

    #[error("Job {id} stuck for {duration:?}")]
    Stuck { id: Uuid, duration: Duration },

    #[error("Maximum parallel jobs ({max}) exceeded")]
    MaxJobsExceeded { max: usize },

    #[error("Job {id} context error: {reason}")]
    ContextError { id: Uuid, reason: String },

    #[error("Job {id} persistence error: {reason}")]
    PersistenceError { id: Uuid, reason: String },
}
