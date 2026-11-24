//! Validation of keyword arguments against a schema.
//!
//! A schema describes which arguments a tool accepts; these checks decide
//! whether one concrete argument object satisfies it. Unknown arguments are
//! rejected outright, whatever the schema declares.

use std::collections::BTreeSet;

use serde_json::Value;

use super::{MAX_SCHEMA_DEPTH, SchemaError, declared_types, value_matches};

/// Validates keyword arguments against a schema already accepted by
/// [`validate_schema`](super::validate_schema).
pub fn validate_arguments(schema: &Value, arguments: &Value) -> Result<(), SchemaError> {
    let object = arguments
        .as_object()
        .ok_or_else(|| SchemaError::new("arguments", "arguments must be a JSON object"))?;
    validate_object(schema, object, "arguments", 0)
}

fn validate_object(
    schema: &Value,
    arguments: &serde_json::Map<String, Value>,
    path: &str,
    depth: usize,
) -> Result<(), SchemaError> {
    check_argument_depth(path, depth)?;
    let object = schema
        .as_object()
        .ok_or_else(|| SchemaError::new(path, "schema must be a JSON object"))?;

    let empty = serde_json::Map::new();
    let properties = object
        .get("properties")
        .and_then(Value::as_object)
        .unwrap_or(&empty);

    reject_unknown_arguments(arguments, properties, path)?;
    check_required_arguments(object, arguments, path)?;
    validate_present_arguments(properties, arguments, path, depth)
}

fn check_argument_depth(path: &str, depth: usize) -> Result<(), SchemaError> {
    if depth > MAX_SCHEMA_DEPTH {
        return Err(SchemaError::new(
            path,
            format!("argument nesting exceeds {MAX_SCHEMA_DEPTH} levels"),
        ));
    }
    Ok(())
}

fn reject_unknown_arguments(
    arguments: &serde_json::Map<String, Value>,
    properties: &serde_json::Map<String, Value>,
    path: &str,
) -> Result<(), SchemaError> {
    for key in arguments.keys() {
        if !properties.contains_key(key) {
            return Err(SchemaError::new(
                format!("{path}.{key}"),
                "unknown keyword argument",
            ));
        }
    }
    Ok(())
}

fn check_required_arguments(
    schema: &serde_json::Map<String, Value>,
    arguments: &serde_json::Map<String, Value>,
    path: &str,
) -> Result<(), SchemaError> {
    for name in required_names(schema) {
        if !arguments.contains_key(name) {
            return Err(SchemaError::new(
                format!("{path}.{name}"),
                "required argument is missing",
            ));
        }
    }
    Ok(())
}

fn required_names(schema: &serde_json::Map<String, Value>) -> BTreeSet<&str> {
    schema
        .get("required")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .collect()
}

fn validate_present_arguments(
    properties: &serde_json::Map<String, Value>,
    arguments: &serde_json::Map<String, Value>,
    path: &str,
    depth: usize,
) -> Result<(), SchemaError> {
    for (name, value) in arguments {
        let Some(property) = properties.get(name) else {
            continue;
        };
        validate_value(property, value, &format!("{path}.{name}"), depth + 1)?;
    }
    Ok(())
}

fn validate_value(
    schema: &Value,
    value: &Value,
    path: &str,
    depth: usize,
) -> Result<(), SchemaError> {
    let object = schema
        .as_object()
        .ok_or_else(|| SchemaError::new(path, "schema must be a JSON object"))?;
    let types = declared_types(object.get("type"), path)?;
    check_declared_type(&types, value, path, depth)?;
    check_value_enum(object, value, path)?;
    validate_object_value(schema, &types, value, path, depth)?;
    validate_array_value(object, &types, value, path, depth)
}

fn check_declared_type(
    types: &[&str],
    value: &Value,
    path: &str,
    depth: usize,
) -> Result<(), SchemaError> {
    if types.is_empty() || types.iter().any(|kind| value_matches(kind, value, depth)) {
        return Ok(());
    }
    Err(SchemaError::new(
        path,
        format!("value does not match declared type {}", types.join(" or ")),
    ))
}

fn check_value_enum(
    object: &serde_json::Map<String, Value>,
    value: &Value,
    path: &str,
) -> Result<(), SchemaError> {
    match object.get("enum").and_then(Value::as_array) {
        Some(values) if !values.contains(value) => {
            Err(SchemaError::new(path, "value is not in the declared enum"))
        }
        _ => Ok(()),
    }
}

fn validate_object_value(
    schema: &Value,
    types: &[&str],
    value: &Value,
    path: &str,
    depth: usize,
) -> Result<(), SchemaError> {
    if !(value.is_object() && types.contains(&"object")) {
        return Ok(());
    }
    let arguments = value
        .as_object()
        .ok_or_else(|| SchemaError::new(path, "value must be an object"))?;
    validate_object(schema, arguments, path, depth)
}

fn validate_array_value(
    object: &serde_json::Map<String, Value>,
    types: &[&str],
    value: &Value,
    path: &str,
    depth: usize,
) -> Result<(), SchemaError> {
    if !(value.is_array() && types.contains(&"array")) {
        return Ok(());
    }
    let items = object
        .get("items")
        .ok_or_else(|| SchemaError::new(path, "array schema lacks items"))?;
    for (index, item) in value.as_array().into_iter().flatten().enumerate() {
        validate_value(items, item, &format!("{path}[{index}]"), depth + 1)?;
    }
    Ok(())
}
