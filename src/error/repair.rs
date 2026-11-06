//! Self-repair error types.

use uuid::Uuid;

/// Self-repair errors.
#[derive(Debug, thiserror::Error)]
pub enum RepairError {
    /// A repair attempt for the target failed.
    #[error("Repair failed for {target_type} {target_id}: {reason}")]
    Failed {
        /// Kind of target, either `tool` or `job`.
        target_type: String,
        /// ID of the target being repaired.
        target_id: Uuid,
        /// Why the repair failed.
        reason: String,
    },

    /// Repair stopped after reaching the attempt limit for the target.
    #[error("Maximum repair attempts ({max}) exceeded for {target_type} {target_id}")]
    MaxAttemptsExceeded {
        /// Kind of target, either `tool` or `job`.
        target_type: String,
        /// ID of the target being repaired.
        target_id: Uuid,
        /// The attempt limit that was reached.
        max: u32,
    },

    /// The target's problem could not be diagnosed, so no repair was attempted.
    #[error("Cannot diagnose issue for {target_type} {target_id}: {reason}")]
    DiagnosisFailed {
        /// Kind of target, either `tool` or `job`.
        target_type: String,
        /// ID of the target that could not be diagnosed.
        target_id: Uuid,
        /// Why diagnosis failed.
        reason: String,
    },
}
