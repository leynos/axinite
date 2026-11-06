//! Evaluation-related error types.

use uuid::Uuid;

/// Evaluation errors.
#[derive(Debug, thiserror::Error)]
pub enum EvaluationError {
    /// The evaluation of a job did not complete.
    #[error("Evaluation failed for job {job_id}: {reason}")]
    Failed {
        /// Job whose evaluation failed.
        job_id: Uuid,
        /// Why the evaluation failed.
        reason: String,
    },

    /// An input the evaluator needs was absent, so no verdict could be reached.
    #[error("Missing required evaluation data: {field}")]
    MissingData {
        /// Name of the absent input.
        field: String,
    },

    /// The evaluation criteria are malformed or inconsistent.
    #[error("Invalid evaluation criteria: {reason}")]
    InvalidCriteria {
        /// What is wrong with the criteria.
        reason: String,
    },
}
