//! Validation of a start request against protocol invariants.
//!
//! These are the parts the helper must never trust: version-shaped limits,
//! identifiers, uniqueness, and reserved names. Schema support is validated by
//! the parent before it builds the request, so it is not repeated here.

use crate::messages::{
    GuestLimits, MAX_DESCRIPTION_BYTES, MAX_IDENTIFIER_BYTES, MAX_RUN_ID_BYTES, MAX_TOOLS,
    StartRun, ToolDefinition,
};
use serde_json::Value;

const PYTHON_KEYWORDS: &[&str] = &[
    "False", "None", "True", "and", "as", "assert", "async", "await", "break", "class", "continue",
    "def", "del", "elif", "else", "except", "finally", "for", "from", "global", "if", "import",
    "in", "is", "lambda", "nonlocal", "not", "or", "pass", "raise", "return", "try", "while",
    "with", "yield",
];

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
pub(crate) fn is_python_identifier(value: &str) -> bool {
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
