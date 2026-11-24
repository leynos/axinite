//! The public `exec_code` tool.
//!
//! The tool validates the caller's request, requires a caller-issued
//! [`BrokerContext`], and hands the run to one supervised helper process. It
//! never links the interpreter and never invokes a tool directly: nested
//! effects go through [`BrokerContext::mediate`].

use std::time::Duration;

use serde_json::{Map, Value, json};
use uuid::Uuid;

use crate::context::JobContext;
use crate::tools::monty::broker::{BrokerContext, BrokerDenial};
use crate::tools::monty::is_python_identifier;
use crate::tools::monty::runner::{HelperProgram, RunError, RunLimits, RunRequest, Supervisor};
use crate::tools::{ApprovalRequirement, HostedToolEligibility, NativeTool, ToolError, ToolOutput};

/// Default wall-clock budget for one run.
pub const DEFAULT_DEADLINE: Duration = Duration::from_secs(120);

/// Hard ceiling on the wall-clock budget a caller may request.
pub const MAX_DEADLINE: Duration = Duration::from_secs(300);

/// The guest heap ceiling the helper enforces.
pub const MAX_GUEST_MEMORY_BYTES: u64 = 64 * 1024 * 1024;

/// The recursion ceiling the helper enforces.
pub const MAX_RECURSION_DEPTH: u64 = 256;

/// Executes bounded Python through the isolated Monty helper.
pub struct ExecCodeTool {
    program: Option<HelperProgram>,
}

impl ExecCodeTool {
    /// Creates the tool using the helper resolution rules.
    pub fn new() -> Self {
        Self {
            program: HelperProgram::resolve().ok(),
        }
    }

    /// Creates the tool with an explicit helper executable.
    pub fn with_helper(path: impl Into<std::path::PathBuf>) -> Self {
        Self {
            program: Some(HelperProgram::at(path)),
        }
    }

    fn parse(&self, params: &Value) -> Result<ParsedRequest, ToolError> {
        let object = params
            .as_object()
            .ok_or_else(|| invalid("arguments must be a JSON object"))?;
        let code = object
            .get("code")
            .and_then(Value::as_str)
            .ok_or_else(|| invalid("code must be a string"))?;
        if code.trim().is_empty() {
            return Err(invalid("code must not be empty"));
        }
        let allowed = parse_allowed(object.get("allowed_tools"))?;
        let entrypoint = object
            .get("entrypoint")
            .and_then(Value::as_str)
            .unwrap_or("main")
            .to_string();
        if !is_python_identifier(&entrypoint) || monty_protocol::is_reserved_entrypoint(&entrypoint)
        {
            return Err(invalid(
                "entrypoint must be a non-reserved Python identifier",
            ));
        }
        let inputs = parse_object(object.get("params"), "params")?;
        let state = parse_object(object.get("state"), "state")?;
        let limits = parse_limits(object.get("limits"))?;
        Ok(ParsedRequest {
            code: code.to_string(),
            allowed,
            entrypoint,
            inputs,
            state,
            limits,
        })
    }
}

impl Default for ExecCodeTool {
    fn default() -> Self {
        Self::new()
    }
}

struct ParsedRequest {
    code: String,
    allowed: Vec<String>,
    entrypoint: String,
    inputs: Value,
    state: Value,
    limits: RunLimits,
}

impl NativeTool for ExecCodeTool {
    fn name(&self) -> &str {
        "exec_code"
    }

    fn description(&self) -> &str {
        "Execute bounded Python in an isolated helper. The program receives \
         params and state, may call only the explicitly allowed tools, and \
         must return an object containing an object `state`."
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "code": {
                    "type": "string",
                    "description": "The body of the async entrypoint function."
                },
                "allowed_tools": {
                    "type": "array",
                    "items": {"type": "string"},
                    "description": "Tool names the program may call. Empty means none."
                },
                // Free-form objects: any members are accepted, but the value
                // must be an object because the helper parses it as one.
                "params": {
                    "type": "object",
                    "properties": {},
                    "description": "Inputs passed as `params`."
                },
                "state": {
                    "type": "object",
                    "properties": {},
                    "description": "Initial state passed as `state`."
                },
                "entrypoint": {
                    "type": "string",
                    "description": "Entrypoint name. Defaults to main."
                },
                "limits": {
                    "type": "object",
                    "properties": {
                        "deadline_ms": {"type": "integer"},
                        "max_memory_bytes": {"type": "integer"},
                        "max_recursion_depth": {"type": "integer"}
                    },
                    "additionalProperties": false
                }
            },
            "required": ["code", "allowed_tools"],
            "additionalProperties": false
        })
    }

    async fn execute(&self, params: Value, ctx: &JobContext) -> Result<ToolOutput, ToolError> {
        let start = std::time::Instant::now();
        let request = self.parse(&params)?;
        let broker = ctx
            .broker
            .as_ref()
            .ok_or_else(|| ToolError::NotAuthorized(BrokerDenial::unavailable().message))?;
        verify_attenuation(broker, &request.allowed)?;
        let program = self.program.clone().ok_or_else(|| {
            ToolError::ExecutionFailed("the Monty helper is not available".into())
        })?;
        let supervisor = Supervisor::with_program(program);
        let outcome = supervisor
            .run(
                RunRequest {
                    run_id: Uuid::new_v4().to_string(),
                    code: request.code,
                    entrypoint: request.entrypoint,
                    params: request.inputs,
                    state: request.state,
                    limits: request.limits,
                },
                broker,
                ctx,
            )
            .await
            .map_err(run_error)?;
        Ok(ToolOutput::success(
            json!({"result": outcome.result, "state": outcome.state}),
            start.elapsed(),
        ))
    }

    fn requires_approval(&self, _params: &Value) -> ApprovalRequirement {
        ApprovalRequirement::UnlessAutoApproved
    }

    fn execution_timeout(&self) -> Duration {
        MAX_DEADLINE + Duration::from_secs(30)
    }

    fn sensitive_params(&self) -> &[&str] {
        &["code", "params", "state"]
    }

    fn hosted_tool_eligibility(&self) -> HostedToolEligibility {
        HostedToolEligibility::ApprovalGated
    }
}

fn verify_attenuation(broker: &BrokerContext, requested: &[String]) -> Result<(), ToolError> {
    let table = broker.table();
    if table.len() != requested.len() {
        return Err(ToolError::NotAuthorized(
            "the caller's broker does not match the requested tools".into(),
        ));
    }
    for name in requested {
        if table.index_of(name).is_none() {
            return Err(ToolError::NotAuthorized(format!(
                "tool {name:?} is not in the caller's broker"
            )));
        }
    }
    Ok(())
}

fn parse_allowed(value: Option<&Value>) -> Result<Vec<String>, ToolError> {
    let array = value
        .and_then(Value::as_array)
        .ok_or_else(|| invalid("allowed_tools must be an array of tool names"))?;
    let mut names = Vec::with_capacity(array.len());
    for entry in array {
        let name = entry
            .as_str()
            .ok_or_else(|| invalid("allowed_tools must contain only strings"))?;
        if names.iter().any(|existing| existing == name) {
            return Err(invalid("allowed_tools contains a duplicate name"));
        }
        names.push(name.to_string());
    }
    Ok(names)
}

fn parse_object(value: Option<&Value>, field: &str) -> Result<Value, ToolError> {
    match value {
        None => Ok(json!({})),
        Some(Value::Object(_)) => Ok(value.cloned().unwrap_or_else(|| json!({}))),
        Some(_) => Err(invalid(format!("{field} must be a JSON object"))),
    }
}

fn parse_limits(value: Option<&Value>) -> Result<RunLimits, ToolError> {
    let mut limits = RunLimits::default();
    let Some(object) = value else {
        return Ok(limits);
    };
    let object = object
        .as_object()
        .ok_or_else(|| invalid("limits must be a JSON object"))?;
    if let Some(deadline) = bounded(object, "deadline_ms", 1, MAX_DEADLINE.as_millis() as u64)? {
        limits.deadline = Duration::from_millis(deadline);
    }
    if let Some(memory) = bounded(object, "max_memory_bytes", 1, MAX_GUEST_MEMORY_BYTES)? {
        limits.max_memory_bytes = memory;
    }
    if let Some(depth) = bounded(object, "max_recursion_depth", 1, MAX_RECURSION_DEPTH)? {
        limits.max_recursion_depth = depth;
    }
    Ok(limits)
}

fn bounded(
    object: &Map<String, Value>,
    field: &str,
    minimum: u64,
    maximum: u64,
) -> Result<Option<u64>, ToolError> {
    let Some(value) = object.get(field) else {
        return Ok(None);
    };
    let number = value
        .as_u64()
        .ok_or_else(|| invalid(format!("limits.{field} must be a positive integer")))?;
    if number < minimum || number > maximum {
        return Err(invalid(format!(
            "limits.{field} must be between {minimum} and {maximum}"
        )));
    }
    Ok(Some(number))
}

fn run_error(error: RunError) -> ToolError {
    match error {
        RunError::ToolDenied(denial) => {
            ToolError::NotAuthorized(format!("{}: {}", denial_name(denial.code), denial.message))
        }
        other => ToolError::ExecutionFailed(other.to_string()),
    }
}

fn denial_name(code: monty_protocol::DenialCode) -> &'static str {
    match code {
        monty_protocol::DenialCode::NotAllowed => "not_allowed",
        monty_protocol::DenialCode::ApprovalRequired => "approval_required",
        monty_protocol::DenialCode::PolicyDenied => "policy_denied",
        monty_protocol::DenialCode::ToolFailed => "tool_failed",
        monty_protocol::DenialCode::ContractError => "contract_error",
        monty_protocol::DenialCode::RecursionDenied => "recursion_denied",
        monty_protocol::DenialCode::BrokerUnavailable => "broker_unavailable",
    }
}

fn invalid(message: impl Into<String>) -> ToolError {
    ToolError::InvalidParameters(message.into())
}

#[cfg(test)]
mod tests {
    //! Unit tests for the `exec_code` tool surface.

    use super::*;

    #[test]
    fn the_default_entrypoint_is_accepted() {
        let request = ExecCodeTool::new()
            .parse(&json!({"code": "return {'state': {}}", "allowed_tools": []}))
            .expect("the default entrypoint is valid");
        assert_eq!(request.entrypoint, "main");
    }

    #[test]
    fn wrapper_reserved_entrypoints_are_rejected() {
        for entrypoint in ["params", "state", "__axinite_call__"] {
            let Err(error) = ExecCodeTool::new().parse(&json!({
                "code": "return {'state': {}}",
                "allowed_tools": [],
                "entrypoint": entrypoint
            })) else {
                panic!("{entrypoint} must be rejected as an entrypoint");
            };
            assert!(
                matches!(error, ToolError::InvalidParameters(_)),
                "{error:?}"
            );
        }
    }
}
