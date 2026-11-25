//! Typed protocol messages.
//!
//! Every message is a JSON object with a `kind` discriminator. Unknown fields,
//! unknown variants and unknown versions are rejected rather than ignored.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::PROTOCOL_VERSION;

mod validation;

pub use validation::{is_reserved_entrypoint, validate_run_id, validate_start_run};

/// Largest number of tools a single run may expose.
pub const MAX_TOOLS: usize = 64;

/// Largest accepted `run_id` in bytes.
pub const MAX_RUN_ID_BYTES: usize = 64;

/// Largest accepted tool or entrypoint identifier in bytes.
pub const MAX_IDENTIFIER_BYTES: usize = 64;

/// Largest accepted tool description in bytes.
pub const MAX_DESCRIPTION_BYTES: usize = 16 * 1024;

/// Limits the parent asks the helper to enforce inside Monty.
///
/// The helper re-validates each field against its own hard ceilings and
/// refuses a request that asks for more.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GuestLimits {
    /// Allocator-backed guest heap ceiling in bytes.
    pub max_memory_bytes: u64,
    /// Maximum Python call-stack depth.
    pub max_recursion_depth: u64,
    /// Maximum host suspensions the helper may service.
    pub max_suspensions: u64,
    /// Maximum execution time for one feed, in milliseconds.
    pub max_feed_millis: u64,
    /// Maximum execution time for one host turn, in milliseconds.
    pub max_turn_millis: u64,
    /// Cumulative guest `print` output ceiling in bytes.
    pub max_output_bytes: u64,
}

/// A tool the guest may request by name.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ToolDefinition {
    /// The callable name exposed inside the guest.
    pub name: String,
    /// A description rendered as the guest function's docstring.
    pub description: String,
    /// The advertised JSON Schema, already validated by the parent.
    pub parameters: Value,
}

/// The helper's first message, sent before it reads source.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Hello {
    /// The protocol version the helper speaks.
    pub protocol_version: u32,
}

/// The parent's request to compile and start one run.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StartRun {
    /// Parent-issued identity for this run.
    pub run_id: String,
    /// The body of the guest's async entrypoint, without the wrapper.
    pub code: String,
    /// The guest entrypoint name.
    pub entrypoint: String,
    /// JSON object passed as `params`.
    pub params: Value,
    /// JSON object passed as `state`.
    pub state: Value,
    /// The selected tools, in stable index order.
    pub tools: Vec<ToolDefinition>,
    /// Guest-side resource ceilings.
    pub limits: GuestLimits,
}

/// A guest request for a host tool call.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HostCall {
    /// The run this callback belongs to.
    pub run_id: String,
    /// Strictly increasing callback identity within the run.
    pub call_id: u64,
    /// The requested tool name.
    pub name: String,
    /// Keyword arguments, always a JSON object.
    pub arguments: Value,
}

/// Which guest stream produced output.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GuestStream {
    /// Standard output.
    Stdout,
    /// Standard error.
    Stderr,
}

/// Bounded guest print output.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GuestOutput {
    /// The run that produced the output.
    pub run_id: String,
    /// The originating stream.
    pub stream: GuestStream,
    /// The captured text.
    pub text: String,
}

/// The parent's answer to one host callback.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "outcome", rename_all = "snake_case", deny_unknown_fields)]
pub enum CallOutcome {
    /// Resume the guest with a JSON value.
    Value {
        /// The validated and scanned tool result.
        value: Value,
    },
    /// Stop the run with a typed host-side denial or failure.
    Terminate {
        /// The failure class.
        code: DenialCode,
        /// A redacted operator-facing message.
        message: String,
    },
}

/// Typed host-side failure classes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DenialCode {
    /// The requested name is outside the run's selected capability view.
    NotAllowed,
    /// The tool requires interactive approval, which v1 cannot mediate.
    ApprovalRequired,
    /// A caller policy rejected the invocation.
    PolicyDenied,
    /// The underlying tool failed or timed out.
    ToolFailed,
    /// A tool result or callback payload violated the JSON contract.
    ContractError,
    /// The guest attempted to re-enter code execution.
    RecursionDenied,
    /// The caller supplied no broker capability.
    BrokerUnavailable,
}

/// The parent's answer to a callback.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ResumeRun {
    /// The run being resumed.
    pub run_id: String,
    /// The callback being answered.
    pub call_id: u64,
    /// The answer.
    pub outcome: CallOutcome,
}

/// The parent's request to cancel a run.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CancelRun {
    /// The run being cancelled.
    pub run_id: String,
}

/// One terminal result for a run.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case", deny_unknown_fields)]
pub enum TerminalOutcome {
    /// The guest returned a valid object containing an object `state`.
    Completed {
        /// The guest's `result` member, or JSON null when omitted.
        result: Value,
        /// The guest's required `state` object.
        state: Value,
    },
    /// The guest raised an exception or returned an invalid value.
    ScriptError {
        /// A redacted summary, never a raw traceback with host data.
        message: String,
    },
    /// A host callback was denied or failed.
    ToolError {
        /// The failure class.
        code: DenialCode,
        /// A redacted summary.
        message: String,
    },
    /// Monty reported an internal execution or memory limit.
    ResourceLimitExceeded {
        /// A redacted summary.
        message: String,
    },
    /// The helper observed an inconsistent or unsupported state.
    RuntimeCrash {
        /// A redacted summary.
        message: String,
    },
}

/// A terminal message for one run.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Terminal {
    /// The run that ended.
    pub run_id: String,
    /// How it ended.
    pub outcome: TerminalOutcome,
}

/// Messages the helper sends to the parent.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum HelperToParent {
    /// The helper is ready; the parent may send `start_run`.
    Hello(Hello),
    /// The guest requested a host tool.
    HostCall(HostCall),
    /// The guest produced bounded print output.
    GuestOutput(GuestOutput),
    /// The run ended.
    Terminal(Terminal),
}

/// Messages the parent sends to the helper.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ParentToHelper {
    /// Compile and start a run.
    StartRun(StartRun),
    /// Answer an outstanding callback.
    ResumeRun(ResumeRun),
    /// Cancel a run.
    CancelRun(CancelRun),
}

/// The helper always answers `hello` with this version.
pub fn hello() -> HelperToParent {
    HelperToParent::Hello(Hello {
        protocol_version: PROTOCOL_VERSION,
    })
}

#[cfg(test)]
mod tests;
