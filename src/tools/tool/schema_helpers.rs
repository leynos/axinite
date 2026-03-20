use super::traits::ToolError;

/// Extract a required string parameter from a JSON object.
///
/// Returns `ToolError::InvalidParameters` if the key is missing or not a string.
pub fn require_str<'a>(params: &'a serde_json::Value, name: &str) -> Result<&'a str, ToolError> {
    params
        .get(name)
        .and_then(|v| v.as_str())
        .ok_or_else(|| ToolError::InvalidParameters(format!("missing '{}' parameter", name)))
}

/// Extract a required parameter of any type from a JSON object.
///
/// Returns `ToolError::InvalidParameters` if the key is missing.
pub fn require_param<'a>(
    params: &'a serde_json::Value,
    name: &str,
) -> Result<&'a serde_json::Value, ToolError> {
    params
        .get(name)
        .ok_or_else(|| ToolError::InvalidParameters(format!("missing '{}' parameter", name)))
}

/// Replace sensitive parameter values with `"[REDACTED]"`.
///
/// Returns a new JSON value with the specified keys replaced. Non-object params
/// and unknown keys are passed through unchanged. The original value is cloned
/// only if there are sensitive params to redact; otherwise it is cloned once
/// (cheap — callers own the result).
///
/// Used by the agent framework before logging, hook dispatch, approval display,
/// and `ActionRecord` storage so plaintext secrets never reach those paths.
pub fn redact_params(params: &serde_json::Value, sensitive: &[&str]) -> serde_json::Value {
    if sensitive.is_empty() {
        return params.clone();
    }
    let mut redacted = params.clone();
    if let Some(obj) = redacted.as_object_mut() {
        for key in sensitive {
            if obj.contains_key(*key) {
                obj.insert(
                    (*key).to_string(),
                    serde_json::Value::String("[REDACTED]".into()),
                );
            }
        }
    }
    redacted
}

/// Lenient runtime validation of a tool's `parameters_schema()`.
///
/// Use this function at tool-registration time to catch structural mistakes
/// (missing `"type": "object"`, orphan `"required"` keys, arrays without
/// `"items"`) without rejecting intentional freeform properties.
///
/// For the stricter variant that also enforces `additionalProperties: false`,
/// enum-type consistency, and per-property `"type"` fields, see
/// [`validate_strict_schema`](crate::tools::schema_validator::validate_strict_schema)
/// in `schema_validator.rs` (used in CI tests).
///
/// Returns a list of validation errors. An empty list means the schema is valid.
///
/// # Rules enforced
///
/// 1. Top-level must have `"type": "object"`
/// 2. Top-level must have `"properties"` as an object
/// 3. Every key in `"required"` must exist in `"properties"`
/// 4. Nested objects follow the same rules recursively
/// 5. Array properties should have `"items"` defined
///
/// Properties without a `"type"` field are allowed (freeform/any-type).
/// This is an intentional pattern used by tools like `json` and `http` for
/// OpenAI compatibility, since union types with arrays require `items`.
pub fn validate_tool_schema(schema: &serde_json::Value, path: &str) -> Vec<String> {
    let mut errors = Vec::new();

    match schema.get("type").and_then(|t| t.as_str()) {
        Some("object") => {}
        Some(other) => {
            errors.push(format!("{path}: expected type \"object\", got \"{other}\""));
            return errors;
        }
        None => {
            errors.push(format!("{path}: missing \"type\": \"object\""));
            return errors;
        }
    }

    let properties = match schema.get("properties").and_then(|p| p.as_object()) {
        Some(p) => p,
        None => {
            errors.push(format!("{path}: missing or non-object \"properties\""));
            return errors;
        }
    };

    if let Some(required) = schema.get("required").and_then(|r| r.as_array()) {
        for req in required {
            if let Some(key) = req.as_str()
                && !properties.contains_key(key)
            {
                errors.push(format!(
                    "{path}: required key \"{key}\" not found in properties"
                ));
            }
        }
    }

    for (key, prop) in properties {
        let prop_path = format!("{path}.{key}");
        if let Some(prop_type) = prop.get("type").and_then(|t| t.as_str()) {
            match prop_type {
                "object" => {
                    errors.extend(validate_tool_schema(prop, &prop_path));
                }
                "array" => {
                    if let Some(items) = prop.get("items") {
                        if items.get("type").and_then(|t| t.as_str()) == Some("object") {
                            errors
                                .extend(validate_tool_schema(items, &format!("{prop_path}.items")));
                        }
                    } else {
                        errors.push(format!("{prop_path}: array property missing \"items\""));
                    }
                }
                _ => {}
            }
        }
    }

    errors
}
