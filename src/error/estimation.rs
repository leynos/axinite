//! Estimation-related error types.

/// Estimation errors.
#[derive(Debug, thiserror::Error)]
pub enum EstimationError {
    #[error("Insufficient data for estimation: need {needed} samples, have {have}")]
    InsufficientData { needed: usize, have: usize },

    #[error("Estimation calculation failed: {reason}")]
    CalculationFailed { reason: String },

    #[error("Invalid estimation parameters: {reason}")]
    InvalidParameters { reason: String },
}
