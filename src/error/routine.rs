//! Routine-related error types.

use super::{DatabaseError, JobError, LlmError};
use uuid::Uuid;

/// Routine-related errors.
#[derive(Debug, thiserror::Error)]
pub enum RoutineError {
    #[error("Unknown trigger type: {trigger_type}")]
    UnknownTriggerType { trigger_type: String },

    #[error("Unknown action type: {action_type}")]
    UnknownActionType { action_type: String },

    #[error("Missing field in {context}: {field}")]
    MissingField { context: String, field: String },

    #[error("Invalid cron expression: {reason}")]
    InvalidCron { reason: String },

    #[error("Unknown run status: {status}")]
    UnknownRunStatus { status: String },

    #[error("Routine {name} is disabled")]
    Disabled { name: String },

    #[error("Routine not found: {id}")]
    NotFound { id: Uuid },

    #[error("Not authorized to trigger routine {id}")]
    NotAuthorized { id: Uuid },

    #[error("Routine {name} at max concurrent runs")]
    MaxConcurrent { name: String },

    #[error(transparent)]
    Database(#[from] DatabaseError),

    #[error(transparent)]
    LlmFailed(#[from] LlmError),

    #[error("Scheduler not available for full-job routine dispatch")]
    SchedulerUnavailable,

    #[error(transparent)]
    JobDispatchFailed(#[from] JobError),

    #[error("LLM returned empty content")]
    EmptyResponse,

    #[error("LLM response truncated (finish_reason=length) with no content")]
    TruncatedResponse,
}
