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

/// Report the first `offender` that `accepts` refuses, or `Ok(())` when it
/// accepts them all. Unknown and missing arguments differ only in which side
/// supplies the names, so both checks are this one.
fn first_refused<'a, T: AsRef<str> + 'a>(
    offenders: impl Iterator<Item = T>,
    accepts: impl Fn(&str) -> bool,
    path: &str,
    message: &str,
) -> Result<(), SchemaError> {
    for offender in offenders {
        let name = offender.as_ref();
        if !accepts(name) {
            return Err(SchemaError::new(
                format!("{path}.{name}"),
                message.to_string(),
            ));
        }
    }
    Ok(())
}

fn reject_unknown_arguments(
    arguments: &serde_json::Map<String, Value>,
    properties: &serde_json::Map<String, Value>,
    path: &str,
) -> Result<(), SchemaError> {
    first_refused(
        arguments.keys(),
        |key| properties.contains_key(key),
        path,
        "unknown keyword argument",
    )
}

fn check_required_arguments(
    schema: &serde_json::Map<String, Value>,
    arguments: &serde_json::Map<String, Value>,
    path: &str,
) -> Result<(), SchemaError> {
    first_refused(
        required_names(schema).into_iter(),
        |name| arguments.contains_key(name),
        path,
        "required argument is missing",
    )
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

/// One value being checked against one schema node, at one path and depth.
///
/// Grouping these keeps each check to the schema and the value it judges,
/// rather than repeating the location on every signature.
struct Check<'a> {
    schema: &'a Value,
    object: &'a serde_json::Map<String, Value>,
    types: Vec<&'a str>,
    value: &'a Value,
    path: &'a str,
    depth: usize,
}

impl<'a> Check<'a> {
    /// Describe one check, reading the declared types from its schema node.
    fn new(
        schema: &'a Value,
        value: &'a Value,
        path: &'a str,
        depth: usize,
    ) -> Result<Self, SchemaError> {
        let object = schema
            .as_object()
            .ok_or_else(|| SchemaError::new(path, "schema must be a JSON object"))?;
        let types = declared_types(object.get("type"), path)?;
        Ok(Self {
            object,
            types,
            schema,
            value,
            path,
            depth,
        })
    }

    /// Judge the value: declared type first, then enum, then the container.
    fn run(&self) -> Result<(), SchemaError> {
        self.check_declared_type()?;
        self.check_enum()?;
        self.validate_object()?;
        self.validate_array()
    }

    fn check_declared_type(&self) -> Result<(), SchemaError> {
        let types = &self.types;
        if types.is_empty()
            || types
                .iter()
                .any(|kind| value_matches(kind, self.value, self.depth))
        {
            return Ok(());
        }
        Err(SchemaError::new(
            self.path,
            format!("value does not match declared type {}", types.join(" or ")),
        ))
    }

    fn check_enum(&self) -> Result<(), SchemaError> {
        match self.object.get("enum").and_then(Value::as_array) {
            Some(values) if !values.contains(self.value) => Err(SchemaError::new(
                self.path,
                "value is not in the declared enum",
            )),
            _ => Ok(()),
        }
    }

    /// Recurse into an object value when the schema declares one.
    fn validate_object(&self) -> Result<(), SchemaError> {
        if !(self.value.is_object() && self.types.contains(&"object")) {
            return Ok(());
        }
        let arguments = self
            .value
            .as_object()
            .ok_or_else(|| SchemaError::new(self.path, "value must be an object"))?;
        validate_object(self.schema, arguments, self.path, self.depth)
    }

    /// Recurse into an array value's items when the schema declares an array.
    fn validate_array(&self) -> Result<(), SchemaError> {
        if !(self.value.is_array() && self.types.contains(&"array")) {
            return Ok(());
        }
        let items = self
            .object
            .get("items")
            .ok_or_else(|| SchemaError::new(self.path, "array schema lacks items"))?;
        for (index, item) in self.value.as_array().into_iter().flatten().enumerate() {
            validate_value(
                items,
                item,
                &format!("{}[{index}]", self.path),
                self.depth + 1,
            )?;
        }
        Ok(())
    }
}

fn validate_value(
    schema: &Value,
    value: &Value,
    path: &str,
    depth: usize,
) -> Result<(), SchemaError> {
    Check::new(schema, value, path, depth)?.run()
}
