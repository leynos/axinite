//! Shared helpers for parameter extraction, redaction, and schema validation.
//!
//! Lightweight newtypes keep schema helper signatures explicit without changing
//! the string values used in validation errors:
//!
//! - [`ParamName`] identifies a JSON parameter key.
//! - [`SchemaPath`] identifies a dot-separated location in a JSON schema.
//! - [`ToolName`] identifies the root tool name used as a schema path in
//!   strict validation.

use super::traits::ToolError;
use serde_json::{Map, Value};
use std::fmt;

macro_rules! impl_borrowed_name_traits {
    ($type_name:ident) => {
        impl fmt::Display for $type_name<'_> {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str(self.0)
            }
        }

        impl AsRef<str> for $type_name<'_> {
            fn as_ref(&self) -> &str {
                self.0
            }
        }

        impl<'a> From<&'a str> for $type_name<'a> {
            fn from(value: &'a str) -> Self {
                Self(value)
            }
        }

        impl<'a> From<&'a String> for $type_name<'a> {
            fn from(value: &'a String) -> Self {
                Self(value.as_str())
            }
        }
    };
}

macro_rules! impl_owned_path_conversions {
    ($type_name:ident) => {
        impl From<&str> for $type_name {
            fn from(value: &str) -> Self {
                Self::new(value)
            }
        }

        impl From<&String> for $type_name {
            fn from(value: &String) -> Self {
                Self::new(value)
            }
        }

        impl From<String> for $type_name {
            fn from(value: String) -> Self {
                Self::new(value)
            }
        }
    };
}

/// A JSON parameter key expected in tool input.
#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub struct ParamName<'a>(&'a str);

impl<'a> ParamName<'a> {
    /// Return the underlying parameter key.
    pub const fn as_str(self) -> &'a str {
        self.0
    }
}

impl_borrowed_name_traits!(ParamName);

/// A dot-separated location in a JSON schema.
#[derive(Debug, Clone, Eq, PartialEq)]
pub struct SchemaPath(String);

impl SchemaPath {
    /// Create a schema path from a root path.
    pub fn new(root: impl Into<String>) -> Self {
        Self(root.into())
    }

    /// Return the underlying schema path.
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Return a child path with `segment` appended after a dot.
    pub fn child(&self, segment: impl AsRef<str>) -> Self {
        Self(format!("{}.{segment}", self.0, segment = segment.as_ref()))
    }
}

impl fmt::Display for SchemaPath {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl AsRef<str> for SchemaPath {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

impl_owned_path_conversions!(SchemaPath);

/// A registered tool identifier used as the root strict-schema path.
#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub struct ToolName<'a>(&'a str);

impl<'a> ToolName<'a> {
    /// Return the underlying tool identifier.
    pub const fn as_str(self) -> &'a str {
        self.0
    }
}

impl_borrowed_name_traits!(ToolName);

impl<'a> From<ToolName<'a>> for SchemaPath {
    fn from(value: ToolName<'a>) -> Self {
        Self::new(value.as_str())
    }
}

/// Extract a required string parameter from a JSON object.
///
/// Returns `ToolError::InvalidParameters` if the key is missing or not a string.
pub fn require_str(params: &serde_json::Value, name: impl AsRef<str>) -> Result<&str, ToolError> {
    let name = name.as_ref();
    params
        .get(name)
        .and_then(|v| v.as_str())
        .ok_or_else(|| ToolError::InvalidParameters(format!("missing '{}' parameter", name)))
}

/// Extract a required parameter of any type from a JSON object.
///
/// Returns `ToolError::InvalidParameters` if the key is missing.
pub fn require_param(
    params: &serde_json::Value,
    name: impl AsRef<str>,
) -> Result<&serde_json::Value, ToolError> {
    let name = name.as_ref();
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

fn is_object_type(schema: &Value) -> bool {
    schema.get("type").and_then(|t| t.as_str()) == Some("object")
}

fn properties_obj(schema: &Value) -> Option<&Map<String, Value>> {
    schema.get("properties").and_then(|p| p.as_object())
}

fn required_array(schema: &Value) -> Option<&Vec<Value>> {
    schema.get("required").and_then(|r| r.as_array())
}

fn validate_required_array(
    required: &[Value],
    properties: &Map<String, Value>,
    path: &SchemaPath,
    out: &mut Vec<String>,
) {
    for req in required {
        if let Some(key) = req.as_str()
            && !properties.contains_key(key)
        {
            out.push(format!(
                "{path}: required key \"{key}\" not found in properties"
            ));
        }
    }
}

fn validate_property_schema(
    name: impl AsRef<str>,
    prop: &Value,
    path: &SchemaPath,
    out: &mut Vec<String>,
) {
    let prop_path = path.child(name);
    if let Some(prop_type) = prop.get("type").and_then(|t| t.as_str()) {
        match prop_type {
            "object" => out.extend(validate_tool_schema_at(prop, &prop_path)),
            "array" => {
                if let Some(items) = prop.get("items") {
                    if items.get("type").and_then(|t| t.as_str()) == Some("object") {
                        let items_path = prop_path.child("items");
                        out.extend(validate_tool_schema_at(items, &items_path));
                    }
                } else {
                    out.push(format!("{prop_path}: array property missing \"items\""));
                }
            }
            _ => {}
        }
    }
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
pub fn validate_tool_schema(
    schema: &serde_json::Value,
    path: impl Into<SchemaPath>,
) -> Vec<String> {
    let path = path.into();
    validate_tool_schema_at(schema, &path)
}

fn validate_tool_schema_at(schema: &serde_json::Value, path: &SchemaPath) -> Vec<String> {
    let mut errors = Vec::new();

    if !is_object_type(schema) {
        match schema.get("type").and_then(|t| t.as_str()) {
            Some(other) => {
                errors.push(format!("{path}: expected type \"object\", got \"{other}\""));
            }
            None => {
                errors.push(format!("{path}: missing \"type\": \"object\""));
            }
        }
        return errors;
    }

    let properties = match properties_obj(schema) {
        Some(p) => p,
        None => {
            errors.push(format!("{path}: missing or non-object \"properties\""));
            return errors;
        }
    };

    if let Some(required) = required_array(schema) {
        validate_required_array(required, properties, path, &mut errors);
    }

    for (key, prop) in properties {
        validate_property_schema(key, prop, path, &mut errors);
    }

    errors
}
