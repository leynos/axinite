//! A deliberately small, fail-closed JSON Schema subset.
//!
//! Tool schemas reach the guest as callables, so the parent must know exactly
//! which keywords it can enforce. Anything outside the subset is rejected
//! before launch rather than silently ignored. The subset is:
//!
//! - `type` (a string or a list of strings) among `string`, `number`,
//!   `integer`, `boolean`, `object`, `array`, `null`;
//! - `properties`, `required`, `additionalProperties: false`;
//! - `items` for arrays;
//! - `enum` for values of the declared type;
//! - the annotations `description`, `title`, `default`, `examples` and
//!   `format`.
//!
//! A property with no `type` is freeform and accepts any bounded JSON value.
//! Unknown keyword arguments are always rejected, whatever the schema says.

use serde_json::Value;

mod arguments;

pub use self::arguments::validate_arguments;

/// A schema or argument validation failure.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{field}: {message}")]
pub struct SchemaError {
    /// The JSON path or keyword that failed.
    pub field: String,
    /// The reason.
    pub message: String,
}

impl SchemaError {
    fn new(field: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            field: field.into(),
            message: message.into(),
        }
    }
}

const MAX_SCHEMA_DEPTH: usize = 16;

const ALLOWED_KEYWORDS: &[&str] = &[
    "type",
    "description",
    "title",
    "default",
    "examples",
    "enum",
    "properties",
    "required",
    "additionalProperties",
    "items",
    "format",
];

const ALLOWED_TYPES: &[&str] = &[
    "string", "number", "integer", "boolean", "object", "array", "null",
];

/// Validates that `schema` is a root object schema inside the supported
/// subset.
pub fn validate_schema(schema: &Value) -> Result<(), SchemaError> {
    let object = schema
        .as_object()
        .ok_or_else(|| SchemaError::new("$", "schema must be a JSON object"))?;
    match object.get("type") {
        Some(Value::String(kind)) if kind == "object" => {}
        Some(_) => {
            return Err(SchemaError::new(
                "$.type",
                "root schema type must be \"object\"",
            ));
        }
        None => {
            return Err(SchemaError::new(
                "$.type",
                "root schema must declare type \"object\"",
            ));
        }
    }
    validate_node(schema, "$", 0)
}

fn validate_node(schema: &Value, path: &str, depth: usize) -> Result<(), SchemaError> {
    check_schema_depth(path, depth)?;
    let object = schema
        .as_object()
        .ok_or_else(|| SchemaError::new(path, "schema must be a JSON object"))?;
    check_keywords(object, path)?;

    let types = declared_types(object.get("type"), path)?;
    check_enum(object, path, &types, depth)?;

    if types.is_empty() {
        return check_freeform(object, path);
    }

    check_type_keywords(object, path, &types, depth)
}

fn check_schema_depth(path: &str, depth: usize) -> Result<(), SchemaError> {
    if depth > MAX_SCHEMA_DEPTH {
        return Err(SchemaError::new(
            path,
            format!("schema nesting exceeds {MAX_SCHEMA_DEPTH} levels"),
        ));
    }
    Ok(())
}

/// Validates the keywords that depend on the declared types.
fn check_type_keywords(
    object: &serde_json::Map<String, Value>,
    path: &str,
    types: &[&str],
    depth: usize,
) -> Result<(), SchemaError> {
    check_object_dispatch(object, path, types, depth)?;
    check_array_keywords(object, path, types, depth)?;
    check_format(object, path)
}

/// Applies the object keywords, or rejects them when no object type is
/// declared.
fn check_object_dispatch(
    object: &serde_json::Map<String, Value>,
    path: &str,
    types: &[&str],
    depth: usize,
) -> Result<(), SchemaError> {
    if types.contains(&"object") {
        return check_object_keywords(object, path, depth);
    }
    reject_present(
        object,
        &["properties", "required", "additionalProperties"],
        path,
        "object keywords require type \"object\"",
    )
}

fn check_keywords(object: &serde_json::Map<String, Value>, path: &str) -> Result<(), SchemaError> {
    for key in object.keys() {
        if !ALLOWED_KEYWORDS.contains(&key.as_str()) {
            return Err(SchemaError::new(
                format!("{path}.{key}"),
                "unsupported schema keyword",
            ));
        }
    }
    Ok(())
}

fn check_enum(
    object: &serde_json::Map<String, Value>,
    path: &str,
    types: &[&str],
    depth: usize,
) -> Result<(), SchemaError> {
    let Some(enum_values) = object.get("enum") else {
        return Ok(());
    };
    let values = enum_values
        .as_array()
        .filter(|values| !values.is_empty())
        .ok_or_else(|| {
            SchemaError::new(format!("{path}.enum"), "enum must be a non-empty array")
        })?;
    for value in values {
        if !types.iter().any(|kind| value_matches(kind, value, depth)) {
            return Err(SchemaError::new(
                format!("{path}.enum"),
                "enum value does not match the declared type",
            ));
        }
    }
    Ok(())
}

/// Rejects constraining keywords on a schema that declares no type.
fn check_freeform(object: &serde_json::Map<String, Value>, path: &str) -> Result<(), SchemaError> {
    reject_present(
        object,
        &[
            "properties",
            "required",
            "additionalProperties",
            "items",
            "enum",
        ],
        path,
        "a schema without a type cannot constrain a value",
    )
}

/// Rejects any of `keywords` that `object` declares.
fn reject_present(
    object: &serde_json::Map<String, Value>,
    keywords: &[&str],
    path: &str,
    message: &'static str,
) -> Result<(), SchemaError> {
    for key in keywords {
        if object.contains_key(*key) {
            return Err(SchemaError::new(format!("{path}.{key}"), message));
        }
    }
    Ok(())
}

/// Validates the keywords that only an object schema may carry.
fn check_object_keywords(
    object: &serde_json::Map<String, Value>,
    path: &str,
    depth: usize,
) -> Result<(), SchemaError> {
    let properties = declared_properties(object, path)?;
    if let Some(properties) = properties {
        for (name, property) in properties {
            validate_node(property, &format!("{path}.properties.{name}"), depth + 1)?;
        }
    }
    if let Some(required) = object.get("required") {
        validate_required(required, properties, path)?;
    }
    match object.get("additionalProperties") {
        None | Some(Value::Bool(false)) => Ok(()),
        Some(_) => Err(SchemaError::new(
            format!("{path}.additionalProperties"),
            "only additionalProperties: false is supported",
        )),
    }
}

fn declared_properties<'a>(
    object: &'a serde_json::Map<String, Value>,
    path: &str,
) -> Result<Option<&'a serde_json::Map<String, Value>>, SchemaError> {
    match object.get("properties") {
        Some(Value::Object(properties)) => Ok(Some(properties)),
        Some(_) => Err(SchemaError::new(
            format!("{path}.properties"),
            "properties must be a JSON object",
        )),
        None => Ok(None),
    }
}

/// Validates the keywords that only an array schema may carry.
fn check_array_keywords(
    object: &serde_json::Map<String, Value>,
    path: &str,
    types: &[&str],
    depth: usize,
) -> Result<(), SchemaError> {
    let Some(items) = object.get("items") else {
        return Ok(());
    };
    if !types.contains(&"array") {
        return Err(SchemaError::new(
            format!("{path}.items"),
            "items requires type \"array\"",
        ));
    }
    validate_node(items, &format!("{path}.items"), depth + 1)
}

fn check_format(object: &serde_json::Map<String, Value>, path: &str) -> Result<(), SchemaError> {
    if let Some(format) = object.get("format")
        && !format.is_string()
    {
        return Err(SchemaError::new(
            format!("{path}.format"),
            "format must be a string",
        ));
    }
    Ok(())
}

fn validate_required(
    required: &Value,
    properties: Option<&serde_json::Map<String, Value>>,
    path: &str,
) -> Result<(), SchemaError> {
    let names = required
        .as_array()
        .ok_or_else(|| SchemaError::new(format!("{path}.required"), "required must be an array"))?;
    for name in names {
        let name = name.as_str().ok_or_else(|| {
            SchemaError::new(
                format!("{path}.required"),
                "required entries must be strings",
            )
        })?;
        if properties.is_none_or(|properties| !properties.contains_key(name)) {
            return Err(SchemaError::new(
                format!("{path}.required"),
                format!("required name '{name}' is not a declared property"),
            ));
        }
    }
    Ok(())
}

fn declared_types<'a>(
    declared: Option<&'a Value>,
    path: &str,
) -> Result<Vec<&'a str>, SchemaError> {
    let types = collect_types(declared, path)?;
    check_type_names(&types, path)?;
    Ok(types)
}

/// Gathers the declared type names without judging whether they are supported.
fn collect_types<'a>(declared: Option<&'a Value>, path: &str) -> Result<Vec<&'a str>, SchemaError> {
    match declared {
        None => Ok(Vec::new()),
        Some(Value::String(kind)) => Ok(vec![kind.as_str()]),
        Some(Value::Array(kinds)) => collect_type_array(kinds, path),
        Some(_) => Err(SchemaError::new(
            format!("{path}.type"),
            "type must be a string or an array of strings",
        )),
    }
}

fn collect_type_array<'a>(kinds: &'a [Value], path: &str) -> Result<Vec<&'a str>, SchemaError> {
    let types = kinds
        .iter()
        .map(|kind| {
            kind.as_str().ok_or_else(|| {
                SchemaError::new(format!("{path}.type"), "type entries must be strings")
            })
        })
        .collect::<Result<Vec<_>, _>>()?;
    if types.is_empty() {
        return Err(SchemaError::new(
            format!("{path}.type"),
            "type array must not be empty",
        ));
    }
    Ok(types)
}

fn check_type_names(types: &[&str], path: &str) -> Result<(), SchemaError> {
    for kind in types {
        if !ALLOWED_TYPES.contains(kind) {
            return Err(SchemaError::new(
                format!("{path}.type"),
                format!("unsupported type '{kind}'"),
            ));
        }
    }
    Ok(())
}

fn value_matches(kind: &str, value: &Value, depth: usize) -> bool {
    if depth > MAX_SCHEMA_DEPTH {
        return false;
    }
    match kind {
        "string" => value.is_string(),
        "number" => value.is_number(),
        "integer" => value.as_i64().is_some(),
        "boolean" => value.is_boolean(),
        "object" => value.is_object(),
        "array" => value.is_array(),
        "null" => value.is_null(),
        _ => false,
    }
}

#[cfg(test)]
mod tests;
