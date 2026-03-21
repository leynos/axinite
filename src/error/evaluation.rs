//! Evaluation-related error types.

use uuid::Uuid;

/// Evaluation errors.
#[derive(Debug, thiserror::Error)]
pub enum EvaluationError {
    #[error("Evaluation failed for job {job_id}: {reason}")]
    Failed { job_id: Uuid, reason: String },

    #[error("Missing required evaluation data: {field}")]
    MissingData { field: String },

    #[error("Invalid evaluation criteria: {reason}")]
    InvalidCriteria { reason: String },
}
