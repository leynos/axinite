//! Estimation-related error types.

/// Estimation errors.
#[derive(Debug, thiserror::Error)]
pub enum EstimationError {
    /// Fewer samples were supplied than the estimate needs.
    #[error("Insufficient data for estimation: need {needed} samples, have {have}")]
    InsufficientData {
        /// Minimum number of samples the estimate requires.
        needed: usize,
        /// Number of samples actually supplied.
        have: usize,
    },

    /// The estimate could not be calculated.
    #[error("Estimation calculation failed: {reason}")]
    CalculationFailed {
        /// Why the calculation could not complete.
        reason: String,
    },

    /// The estimation parameters were rejected.
    #[error("Invalid estimation parameters: {reason}")]
    InvalidParameters {
        /// Why the parameters were rejected.
        reason: String,
    },
}
