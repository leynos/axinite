//! Routine-related error types.

use super::{DatabaseError, JobError, LlmError};
use uuid::Uuid;

/// Routine-related errors.
#[derive(Debug, thiserror::Error)]
pub enum RoutineError {
    /// Returned when a trigger type string cannot be parsed into a known kind.
    #[error("Unknown trigger type: {trigger_type}")]
    UnknownTriggerType {
        /// The unrecognised trigger type string, exactly as supplied.
        trigger_type: String,
    },

    /// Returned when an action type string cannot be parsed into a known kind.
    #[error("Unknown action type: {action_type}")]
    UnknownActionType {
        /// The unrecognised action type string, exactly as supplied.
        action_type: String,
    },

    /// Returned when `field` is missing from the given `context` object/path.
    #[error("Missing field in {context}: {field}")]
    MissingField {
        /// The object or path that was searched, such as a config section name.
        context: String,
        /// The required field that was absent or not a string.
        field: String,
    },

    /// Returned when cron parsing fails for the supplied `reason`.
    #[error("Invalid cron expression: {reason}")]
    InvalidCron {
        /// The cron parser's error message.
        reason: String,
    },

    /// Returned when a stored routine run-state string is not recognized.
    #[error("Unknown run status: {status}")]
    UnknownRunStatus {
        /// The stored run-state string that was not recognised.
        status: String,
    },

    /// Returned when the routine named `name` is disabled.
    #[error("Routine {name} is disabled")]
    Disabled {
        /// Name of the disabled routine.
        name: String,
    },

    /// Returned when no routine exists for the given routine `id`.
    #[error("Routine not found: {id}")]
    NotFound {
        /// The routine UUID that matched no stored routine.
        id: Uuid,
    },

    /// Returned when the caller lacks permission to access routine `id`.
    #[error("Not authorized to trigger routine {id}")]
    NotAuthorized {
        /// UUID of the routine the caller may not access.
        id: Uuid,
    },

    /// Returned when the routine named `name` has reached its concurrency cap.
    #[error("Routine {name} at max concurrent runs")]
    MaxConcurrent {
        /// Name of the routine at its concurrency cap.
        name: String,
    },

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
