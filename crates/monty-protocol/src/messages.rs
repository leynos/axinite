//! Typed protocol messages.
//!
//! Every message is a JSON object with a `kind` discriminator. Unknown fields,
//! unknown variants and unknown versions are rejected rather than ignored.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::PROTOCOL_VERSION;

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

/// Validates a start request against protocol invariants.
///
/// Schema support is validated by the parent before it builds the request;
/// this function checks the parts the helper must never trust: version-shaped
/// limits, identifiers, uniqueness, and reserved names.
pub fn validate_start_run(request: &StartRun) -> Result<(), String> {
    validate_run_id(&request.run_id)?;
    validate_entrypoint(&request.entrypoint)?;
    validate_object_field(&request.params, "params")?;
    validate_object_field(&request.state, "state")?;
    validate_tool_count(request.tools.len())?;
    validate_limits(&request.limits)?;
    validate_tools(&request.tools, &request.entrypoint)
}

/// Require an entrypoint that the wrapper can define and call.
fn validate_entrypoint(entrypoint: &str) -> Result<(), String> {
    if !is_python_identifier(entrypoint) {
        return Err(format!(
            "entrypoint '{entrypoint}' is not a permitted Python identifier"
        ));
    }
    if is_reserved_entrypoint(entrypoint) {
        return Err(format!(
            "entrypoint '{entrypoint}' is reserved by the exec_code wrapper"
        ));
    }
    Ok(())
}

/// Require a JSON object where the ABI passes one.
fn validate_object_field(value: &Value, name: &str) -> Result<(), String> {
    if value.is_object() {
        return Ok(());
    }
    Err(format!("{name} must be a JSON object"))
}

/// Require no more tools than the wire contract carries.
fn validate_tool_count(count: usize) -> Result<(), String> {
    if count > MAX_TOOLS {
        return Err(format!("at most {MAX_TOOLS} tools may be selected"));
    }
    Ok(())
}

/// Require every limit to be positive and within its ceiling.
fn validate_limits(limits: &GuestLimits) -> Result<(), String> {
    for (name, value, ceiling) in [
        (
            "max_memory_bytes",
            limits.max_memory_bytes,
            512 * 1024 * 1024,
        ),
        ("max_recursion_depth", limits.max_recursion_depth, 1000),
        ("max_suspensions", limits.max_suspensions, 10_000),
        ("max_feed_millis", limits.max_feed_millis, 300_000),
        ("max_turn_millis", limits.max_turn_millis, 300_000),
        ("max_output_bytes", limits.max_output_bytes, 1024 * 1024),
    ] {
        if value == 0 || value > ceiling {
            return Err(format!("{name} must be between 1 and {ceiling}"));
        }
    }
    Ok(())
}

/// Require each tool to be well named, unique, and not the entrypoint.
fn validate_tools(tools: &[ToolDefinition], entrypoint: &str) -> Result<(), String> {
    let mut seen = std::collections::BTreeSet::new();
    for tool in tools {
        validate_tool(tool, entrypoint)?;
        if !seen.insert(tool.name.as_str()) {
            return Err(format!("duplicate tool name '{}'", tool.name));
        }
    }
    Ok(())
}

/// Require one tool's name and description to be within contract.
fn validate_tool(tool: &ToolDefinition, entrypoint: &str) -> Result<(), String> {
    validate_identifier("tool name", &tool.name)?;
    if is_reserved_name(&tool.name) || tool.name == entrypoint {
        return Err(format!("tool name '{}' is reserved", tool.name));
    }
    if tool.description.len() > MAX_DESCRIPTION_BYTES {
        return Err(format!(
            "description for '{}' exceeds {MAX_DESCRIPTION_BYTES} bytes",
            tool.name
        ));
    }
    Ok(())
}

/// Validates a parent-issued run identity.
pub fn validate_run_id(run_id: &str) -> Result<(), String> {
    if run_id.is_empty() || run_id.len() > MAX_RUN_ID_BYTES {
        return Err(format!("run_id must be 1..={MAX_RUN_ID_BYTES} bytes"));
    }
    if !run_id
        .bytes()
        .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
    {
        return Err("run_id may contain only ASCII alphanumerics, '-' and '_'".to_owned());
    }
    Ok(())
}

fn validate_identifier(what: &str, value: &str) -> Result<(), String> {
    if value.len() > MAX_IDENTIFIER_BYTES {
        return Err(format!(
            "{what} '{value}' exceeds {MAX_IDENTIFIER_BYTES} bytes"
        ));
    }
    if !is_python_identifier(value) {
        return Err(format!("{what} '{value}' is not a Python identifier"));
    }
    Ok(())
}

fn is_reserved_name(name: &str) -> bool {
    name == "params"
        || name == "state"
        || name.starts_with("__axinite")
        || PYTHON_KEYWORDS.contains(&name)
}

/// Whether `name` is reserved as the guest entrypoint.
///
/// The generated wrapper binds `params` and `state` and keeps the `__axinite`
/// prefix for its own helpers, so a caller may not use them as the entrypoint
/// name. The default entrypoint `main` is deliberately *not* reserved: it is
/// the wrapper's own function name.
pub fn is_reserved_entrypoint(name: &str) -> bool {
    name == "params" || name == "state" || name.starts_with("__axinite")
}

/// Whether `value` is a non-keyword Python identifier.
pub fn is_python_identifier(value: &str) -> bool {
    let mut chars = value.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    if !(first == '_' || first.is_alphabetic()) {
        return false;
    }
    if !chars.all(|ch| ch == '_' || ch.is_alphanumeric()) {
        return false;
    }
    !PYTHON_KEYWORDS.contains(&value)
}

/// The helper always answers `hello` with this version.
pub fn hello() -> HelperToParent {
    HelperToParent::Hello(Hello {
        protocol_version: PROTOCOL_VERSION,
    })
}

const PYTHON_KEYWORDS: &[&str] = &[
    "False", "None", "True", "and", "as", "assert", "async", "await", "break", "class", "continue",
    "def", "del", "elif", "else", "except", "finally", "for", "from", "global", "if", "import",
    "in", "is", "lambda", "nonlocal", "not", "or", "pass", "raise", "return", "try", "while",
    "with", "yield",
];

#[cfg(test)]
mod tests;
