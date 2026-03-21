//! Routine-related error types.

use super::{DatabaseError, JobError, LlmError};
use uuid::Uuid;

/// Routine-related errors.
#[derive(Debug, thiserror::Error)]
pub enum RoutineError {
    /// Returned when a trigger type string cannot be parsed into a known kind.
    #[error("Unknown trigger type: {trigger_type}")]
    UnknownTriggerType { trigger_type: String },

    /// Returned when an action type string cannot be parsed into a known kind.
    #[error("Unknown action type: {action_type}")]
    UnknownActionType { action_type: String },

    /// Returned when `field` is missing from the given `context` object/path.
    #[error("Missing field in {context}: {field}")]
    MissingField { context: String, field: String },

    /// Returned when cron parsing fails for the supplied `reason`.
    #[error("Invalid cron expression: {reason}")]
    InvalidCron { reason: String },

    /// Returned when a stored routine run-state string is not recognised.
    #[error("Unknown run status: {status}")]
    UnknownRunStatus { status: String },

    /// Returned when the routine named `name` is disabled.
    #[error("Routine {name} is disabled")]
    Disabled { name: String },

    /// Returned when no routine exists for the given routine `id`.
    #[error("Routine not found: {id}")]
    NotFound { id: Uuid },

    /// Returned when the caller lacks permission to access routine `id`.
    #[error("Not authorized to trigger routine {id}")]
    NotAuthorized { id: Uuid },

    /// Returned when the routine named `name` has reached its concurrency cap.
    #[error("Routine {name} at max concurrent runs")]
    MaxConcurrent { name: String },

    /// Returned when routine persistence fails in the database layer.
    #[error(transparent)]
    Database(#[from] DatabaseError),

    /// Returned when the routine's LLM interaction fails.
    #[error(transparent)]
    LlmFailed(#[from] LlmError),

    /// Returned when full-job routine dispatch is requested without a scheduler.
    #[error("Scheduler not available for full-job routine dispatch")]
    SchedulerUnavailable,

    /// Returned when dispatching the routine-created job fails.
    #[error(transparent)]
    JobDispatchFailed(#[from] JobError),

    /// Returned when the LLM produced no response content.
    #[error("LLM returned empty content")]
    EmptyResponse,

    /// Returned when the LLM stops for length without usable content.
    #[error("LLM response truncated (finish_reason=length) with no content")]
    TruncatedResponse,
}
