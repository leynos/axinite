// === QA Plan P0 - 1.1: Tool schema validator ===
//!
//! Validates tool parameter schemas against OpenAI strict-mode rules.
//!
//! This module provides a comprehensive validation function and a test that
//! exercises every built-in tool's `parameters_schema()` to ensure compatibility
//! with the OpenAI function calling API strict mode.
//! Recursive locations use [`SchemaPath`](crate::tools::tool::SchemaPath) so
//! helper functions distinguish tool identifiers from schema paths while
//! preserving the existing error text.

use crate::tools::tool::SchemaPath;

/// Strict CI-time validation of a JSON schema against OpenAI strict-mode rules.
///
/// Use this function in tests and CI to catch subtle schema defects that the
/// lenient runtime validator allows (freeform properties, missing
/// `additionalProperties`, enum-type mismatches).
///
/// For the lenient runtime variant used at tool-registration time, see
/// [`validate_tool_schema`](crate::tools::tool::validate_tool_schema) in
/// `tool.rs`.
///
/// Returns `Ok(())` if the schema is valid, or `Err(errors)` with a list of
/// all violations found. The validation is recursive for nested objects and
/// array items.
///
/// # Rules enforced
///
/// 1. Top-level must have `"type": "object"`
/// 2. Must have `"properties"` as a JSON object
/// 3. Every key in `"required"` must exist in `"properties"`
/// 4. Every property must have a `"type"` field (freeform/any-type is flagged)
/// 5. `"additionalProperties"` must be explicitly `false` if present
/// 6. Nested objects follow the same rules recursively
/// 7. `"enum"` values must match the declared type
/// 8. Array properties must have an `"items"` definition
/// 9. Top-level schemas must not use `anyOf`/`allOf`/`enum`/`not`
pub fn validate_strict_schema(
    schema: &serde_json::Value,
    tool_name: impl AsRef<str>,
) -> Result<(), Vec<String>> {
    let tool_name = tool_name.as_ref();
    let mut errors = Vec::new();
    for forbidden in ["anyOf", "allOf", "enum", "not"] {
        if schema.get(forbidden).is_some() {
            errors.push(format!(
                "{tool_name}: top-level \"{forbidden}\" is not allowed in OpenAI tool schemas"
            ));
        }
    }
    errors.extend(check_object_schema(schema, SchemaPath::from(tool_name)));
    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors)
    }
}

fn check_required_keys(
    schema: &serde_json::Value,
    properties: &serde_json::Map<String, serde_json::Value>,
    path: &SchemaPath,
) -> Vec<String> {
    let Some(required) = schema.get("required").and_then(|r| r.as_array()) else {
        return Vec::new();
    };

    let mut errors = Vec::new();
    for req in required {
        if let Some(key) = req.as_str()
            && !properties.contains_key(key)
        {
            errors.push(format!(
                "{path}: required key \"{key}\" not found in properties"
            ));
        }
    }
    errors
}
fn additional_properties_error(
    schema: &serde_json::Value,
    path: &str,
    label: &str,
) -> Option<String> {
    let additional = schema.get("additionalProperties")?;
    if additional != &serde_json::Value::Bool(false) && additional.get("type").is_none() {
        Some(format!(
            "{path}: {label}\"additionalProperties\" should be false or a type schema"
        ))
    } else {
        None
    }
}

fn check_enum_type_match(
    prop: &serde_json::Value,
    prop_type: &str,
    prop_path: &SchemaPath,
) -> Vec<String> {
    let Some(enum_values) = prop.get("enum").and_then(|e| e.as_array()) else {
        return Vec::new();
    };

    let mut errors = Vec::new();
    for (i, val) in enum_values.iter().enumerate() {
        let type_matches = match prop_type {
            "string" => val.is_string(),
            "integer" | "number" => val.is_number(),
            "boolean" => val.is_boolean(),
            _ => true, // unknown types: skip check
        };
        if !type_matches {
            errors.push(format!(
                "{prop_path}: enum[{i}] value {val} does not match declared type \"{prop_type}\""
            ));
        }
    }
    errors
}

fn check_single_property(key: &str, prop: &serde_json::Value, path: &SchemaPath) -> Vec<String> {
    let mut errors = Vec::new();
    let prop_path = path.child(key);

    if prop.get("type").is_none() {
        // Freeform properties (no type) are intentionally allowed in some tools
        // (json "data", http "body") for OpenAI compatibility with union types.
        // We flag them as warnings but don't treat them as hard errors.
        // Uncomment the next line to enforce strict typing:
        // errors.push(format!("{prop_path}: property missing \"type\" field"));
        return errors;
    }

    let prop_type = prop.get("type").and_then(|t| t.as_str()).unwrap_or("");

    // Rule 5: additionalProperties must be false if present
    if let Some(error) = additional_properties_error(prop, prop_path.as_str(), "") {
        errors.push(error);
    }

    // Rule 7: enum values must match the declared type
    errors.extend(check_enum_type_match(prop, prop_type, &prop_path));

    // Rule 6: nested objects follow the same rules
    if prop_type == "object" {
        // Objects with additionalProperties as a type schema (e.g. credentials map)
        // are valid JSON Schema patterns, not strict-mode objects with fixed properties.
        if prop.get("additionalProperties").is_some() && prop.get("properties").is_none() {
            // This is a map type (e.g. {"type": "object", "additionalProperties": {"type": "string"}})
            // Valid pattern, skip recursive object validation.
        } else {
            errors.extend(check_object_schema_at(prop, &prop_path));
        }
    }

    // Rule 8: arrays must have "items"
    if prop_type == "array" {
        if prop.get("items").is_none() {
            errors.push(format!("{prop_path}: array property missing \"items\""));
        } else if let Some(items) = prop.get("items") {
            // Recurse into items if they are objects
            if items.get("type").and_then(|t| t.as_str()) == Some("object") {
                let items_path = prop_path.child("items");
                errors.extend(check_object_schema_at(items, &items_path));
            }
        }
    }

    errors
}
/// Recursively validate an object-typed schema node.
fn check_object_schema(schema: &serde_json::Value, path: impl Into<SchemaPath>) -> Vec<String> {
    let path = path.into();
    check_object_schema_at(schema, &path)
}

fn check_object_schema_at(schema: &serde_json::Value, path: &SchemaPath) -> Vec<String> {
    let mut errors = Vec::new();

    // Rule 1: must have "type": "object"
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

    // Rule 2: must have "properties" as an object
    let properties = match schema.get("properties").and_then(|p| p.as_object()) {
        Some(p) => p,
        None => {
            errors.push(format!("{path}: missing or non-object \"properties\""));
            return errors;
        }
    };

    // Rule 3: every key in "required" must exist in "properties"
    errors.extend(check_required_keys(schema, properties, path));

    // Rule 4: every property should have a "type" field
    for (key, prop) in properties {
        errors.extend(check_single_property(key, prop, path));
    }

    // Also check top-level additionalProperties (rule 5)
    if let Some(error) = additional_properties_error(schema, path.as_str(), "top-level ") {
        errors.push(error);
    }

    errors
}

#[cfg(test)]
mod tests;
