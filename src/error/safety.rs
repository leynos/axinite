//! Safety and sanitization error types.

/// Safety/sanitization errors.
#[derive(Debug, thiserror::Error)]
pub enum SafetyError {
    /// Raised when prompt-injection detection matches the given `pattern`.
    #[error("Potential prompt injection detected: {pattern}")]
    InjectionDetected {
        /// The injection pattern that matched.
        pattern: String,
    },

    /// Raised when content length `length` exceeds the configured `max`.
    #[error("Output exceeded maximum length: {length} > {max}")]
    OutputTooLarge {
        /// Measured length of the rejected output. The unit is set by the caller.
        length: usize,
        /// The configured limit that `length` exceeded.
        max: usize,
    },

    /// Raised when blocked output matches the given `pattern`.
    #[error("Blocked content pattern detected: {pattern}")]
    BlockedContent {
        /// The blocked pattern that matched the output.
        pattern: String,
    },

    /// Raised when a safety validation fails for the given `reason`.
    #[error("Validation failed: {reason}")]
    ValidationFailed {
        /// Why the validation failed.
        reason: String,
    },

    /// Raised when processing violates the named safety `rule`.
    #[error("Policy violation: {rule}")]
    PolicyViolation {
        /// Name of the safety rule that was violated.
        rule: String,
    },
}
