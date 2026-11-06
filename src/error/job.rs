//! Job-related error types.

use std::time::Duration;

use uuid::Uuid;

/// Job-related errors.
#[derive(Debug, thiserror::Error)]
pub enum JobError {
    /// No job exists with the requested ID.
    #[error("Job {id} not found")]
    NotFound {
        /// ID of the job that was looked up.
        id: Uuid,
    },

    /// A state change was refused because the job is not in a state that allows it.
    #[error("Job {id} already in state {from_state}, cannot transition to {target}")]
    InvalidTransition {
        /// Job whose transition was refused.
        id: Uuid,
        /// State the job was in when the transition was attempted.
        from_state: crate::context::JobState,
        /// State the job was asked to move to.
        target: crate::context::JobState,
    },

    /// The job ended in failure.
    #[error("Job {id} failed: {reason}")]
    Failed {
        /// Job that failed.
        id: Uuid,
        /// Human-readable reason for the failure.
        reason: String,
    },

    /// The job is stuck and has been so for `duration`.
    #[error("Job {id} stuck for {duration:?}")]
    Stuck {
        /// Job that is stuck.
        id: Uuid,
        /// How long the job has been stuck.
        duration: Duration,
    },

    /// Starting another job would exceed the parallel job limit.
    #[error("Maximum parallel jobs ({max}) exceeded")]
    MaxJobsExceeded {
        /// Configured maximum number of parallel jobs.
        max: usize,
    },

    /// The job's context could not be read or updated.
    #[error("Job {id} context error: {reason}")]
    ContextError {
        /// Job whose context failed.
        id: Uuid,
        /// Message describing the context failure.
        reason: String,
    },

    /// The job's state could not be written to or read from the database.
    #[error("Job {id} persistence error: {reason}")]
    PersistenceError {
        /// Job whose persistence failed.
        id: Uuid,
        /// Message describing the persistence failure.
        reason: String,
    },
}
