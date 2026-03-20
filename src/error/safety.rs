//! Safety and sanitization error types.

/// Safety/sanitization errors.
#[derive(Debug, thiserror::Error)]
pub enum SafetyError {
    /// Raised when prompt-injection detection matches the given `pattern`.
    #[error("Potential prompt injection detected: {pattern}")]
    InjectionDetected { pattern: String },

    /// Raised when content length `length` exceeds the configured `max`.
    #[error("Output exceeded maximum length: {length} > {max}")]
    OutputTooLarge { length: usize, max: usize },

    /// Raised when blocked output matches the given `pattern`.
    #[error("Blocked content pattern detected: {pattern}")]
    BlockedContent { pattern: String },

    /// Raised when a safety validation fails for the given `reason`.
    #[error("Validation failed: {reason}")]
    ValidationFailed { reason: String },

    /// Raised when processing violates the named safety `rule`.
    #[error("Policy violation: {rule}")]
    PolicyViolation { rule: String },
}
