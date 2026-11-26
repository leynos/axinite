//! The values a run is described and reported with.
//!
//! `RunRequest` is what the parent asks for; `RunLimits` bounds it; `RunOutcome`
//! and `RunError` are what the supervisor answers with.

use std::time::Duration;

use monty_protocol::GuestLimits;
use serde_json::Value;

use super::{BrokerDenial, MAX_CAPTURED_OUTPUT_BYTES};

/// Limits the parent asks the helper to enforce, plus its own deadline.
#[derive(Debug, Clone, Copy)]
pub struct RunLimits {
    /// Guest allocator ceiling.
    pub max_memory_bytes: u64,
    /// Python call-stack depth.
    pub max_recursion_depth: u64,
    /// Host suspensions the run may service.
    pub max_suspensions: u64,
    /// One feed's execution budget.
    pub max_feed_millis: u64,
    /// One host turn's execution budget.
    pub max_turn_millis: u64,
    /// Cumulative guest output.
    pub max_output_bytes: u64,
    /// Total wall-clock budget from admission.
    pub deadline: Duration,
}

impl Default for RunLimits {
    fn default() -> Self {
        Self {
            max_memory_bytes: 64 * 1024 * 1024,
            max_recursion_depth: 256,
            max_suspensions: 4096,
            max_feed_millis: 30_000,
            max_turn_millis: 30_000,
            max_output_bytes: MAX_CAPTURED_OUTPUT_BYTES as u64,
            deadline: Duration::from_secs(120),
        }
    }
}

impl RunLimits {
    pub(super) fn guest(&self) -> GuestLimits {
        GuestLimits {
            max_memory_bytes: self.max_memory_bytes,
            max_recursion_depth: self.max_recursion_depth,
            max_suspensions: self.max_suspensions,
            max_feed_millis: self.max_feed_millis,
            max_turn_millis: self.max_turn_millis,
            max_output_bytes: self.max_output_bytes,
        }
    }
}

/// Everything the helper needs to start one run.
#[derive(Debug, Clone)]
pub struct RunRequest {
    /// Parent-issued run identity.
    pub run_id: String,
    /// The guest entrypoint body.
    pub code: String,
    /// The entrypoint name.
    pub entrypoint: String,
    /// Input parameters.
    pub params: Value,
    /// Initial state object.
    pub state: Value,
    /// Guest-side limits.
    pub limits: RunLimits,
}

/// A successful run's observable result.
#[derive(Debug, Clone, PartialEq)]
pub struct RunOutcome {
    /// The guest's result member, or null.
    pub result: Value,
    /// The guest's returned state object.
    pub state: Value,
    /// Bounded guest print output.
    pub output: String,
}

/// Why a run did not produce a result.
#[derive(Debug, thiserror::Error)]
pub enum RunError {
    /// The helper executable could not be found.
    #[error("the Monty helper is not available")]
    HelperUnavailable,
    /// Admission is saturated.
    #[error("too many code runs are already active")]
    Overloaded,
    /// The process could not be started.
    #[error("cannot start the Monty helper")]
    Spawn(#[source] std::io::Error),
    /// The helper broke the protocol or exited unexpectedly.
    #[error("the Monty helper failed: {0}")]
    RuntimeCrash(String),
    /// The guest raised an exception.
    #[error("the guest script failed: {0}")]
    ScriptError(String),
    /// A host callback was refused.
    #[error("a tool callback was refused: {0}")]
    ToolDenied(BrokerDenial),
    /// Monty reported an internal limit.
    #[error("the guest exceeded a resource limit: {0}")]
    ResourceLimitExceeded(String),
    /// The run exceeded its wall-clock budget.
    #[error("the code run exceeded its deadline")]
    Timeout,
}
