//! Conversion between the bounded JSON wire values and Monty values.
//!
//! Only JSON null, booleans, signed 64-bit integers, finite binary64 floats,
//! strings, arrays and string-keyed objects cross the boundary. Everything
//! else is rejected rather than coerced.

use std::fmt;

use monty_protocol::MAX_JSON_DEPTH;
use monty_types::{CallArgs, MontyObject, ObjectRef};
use serde_json::{Map, Value};

/// A value that cannot be represented in the JSON ABI.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConvertError {
    message: String,
}

impl ConvertError {
    fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }

    /// The operator-facing reason.
    pub fn message(&self) -> &str {
        &self.message
    }
}

impl fmt::Display for ConvertError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

/// Converts a JSON value into a Monty value.
pub fn json_to_monty(value: &Value, depth: usize) -> Result<MontyObject, ConvertError> {
    if depth > MAX_JSON_DEPTH {
        return Err(ConvertError::new("value exceeds the JSON nesting limit"));
    }
    match value {
        Value::Null => Ok(MontyObject::none()),
        Value::Bool(value) => Ok(MontyObject::bool(*value)),
        Value::Number(number) => {
            if let Some(value) = number.as_i64() {
                Ok(MontyObject::int(value))
            } else if let Some(value) = number.as_f64() {
                if value.is_finite() {
                    Ok(MontyObject::float(value))
                } else {
                    Err(ConvertError::new("float is not finite"))
                }
            } else {
                Err(ConvertError::new(
                    "number is outside the signed 64-bit range",
                ))
            }
        }
        Value::String(value) => Ok(MontyObject::string(value.clone())),
        Value::Array(items) => {
            let items = items
                .iter()
                .map(|item| json_to_monty(item, depth + 1))
                .collect::<Result<Vec<_>, _>>()?;
            Ok(MontyObject::list(items))
        }
        Value::Object(entries) => {
            let pairs = entries
                .iter()
                .map(|(key, value)| {
                    Ok((
                        MontyObject::string(key.clone()),
                        json_to_monty(value, depth + 1)?,
                    ))
                })
                .collect::<Result<Vec<_>, ConvertError>>()?;
            Ok(MontyObject::dict(pairs))
        }
    }
}

/// Converts a Monty value into a JSON value.
pub fn monty_to_json(value: ObjectRef<'_>, depth: usize) -> Result<Value, ConvertError> {
    if depth > MAX_JSON_DEPTH {
        return Err(ConvertError::new("value exceeds the JSON nesting limit"));
    }
    if value == MontyObject::none() {
        return Ok(Value::Null);
    }
    if let Some(value) = value.as_bool() {
        return Ok(Value::Bool(value));
    }
    if let Some(value) = value.as_int() {
        return Ok(Value::Number(value.into()));
    }
    if let Some(value) = value.as_float() {
        return if value.is_finite() {
            serde_json::Number::from_f64(value)
                .map(Value::Number)
                .ok_or_else(|| ConvertError::new("float is not finite"))
        } else {
            Err(ConvertError::new("float is not finite"))
        };
    }
    if let Some(value) = value.as_str() {
        return Ok(Value::String(value.to_owned()));
    }
    match value.type_name() {
        "list" | "tuple" => {
            let items = value
                .items()
                .ok_or_else(|| ConvertError::new("list value has no items"))?;
            let items = items
                .into_iter()
                .map(|item| monty_to_json(item, depth + 1))
                .collect::<Result<Vec<_>, _>>()?;
            Ok(Value::Array(items))
        }
        "dict" => {
            let pairs = value
                .pairs()
                .ok_or_else(|| ConvertError::new("dict value has no items"))?;
            let mut object = Map::new();
            for (key, item) in pairs {
                let key = key.as_str().ok_or_else(|| {
                    ConvertError::new("object keys must be strings in the JSON ABI")
                })?;
                object.insert(key.to_owned(), monty_to_json(item, depth + 1)?);
            }
            Ok(Value::Object(object))
        }
        other => Err(ConvertError::new(format!(
            "value of type '{other}' has no JSON representation"
        ))),
    }
}

/// Converts callback arguments into a JSON object of keyword arguments.
///
/// Positional arguments are rejected: tool schemas are keyword-only.
pub fn call_arguments(args: &CallArgs) -> Result<Value, ConvertError> {
    if args.args().len() != 0 {
        return Err(ConvertError::new(
            "tool callbacks must use keyword arguments only",
        ));
    }
    let mut object = Map::new();
    for (key, value) in args.kwargs() {
        let key = key
            .as_str()
            .ok_or_else(|| ConvertError::new("tool keyword names must be strings"))?;
        object.insert(key.to_owned(), monty_to_json(value, 0)?);
    }
    Ok(Value::Object(object))
}

/// Extracts the guest's `result` and `state` members from a returned object.
///
/// The returned value must be an object with an object `state`; `result`
/// defaults to JSON null when omitted.
pub fn completed_members(value: &MontyObject) -> Result<(Value, Value), ConvertError> {
    let value = value.as_ref();
    if value.type_name() != "dict" {
        return Err(ConvertError::new(
            "the entrypoint must return a JSON object",
        ));
    }
    let pairs = value
        .pairs()
        .ok_or_else(|| ConvertError::new("returned object has no members"))?;
    let mut result = Value::Null;
    let mut state = None;
    for (key, member) in pairs {
        match key.as_str() {
            Some("result") => result = monty_to_json(member, 0)?,
            Some("state") => {
                if member.type_name() != "dict" {
                    return Err(ConvertError::new("returned 'state' must be an object"));
                }
                state = Some(monty_to_json(member, 0)?);
            }
            _ => {}
        }
    }
    let state = state.ok_or_else(|| ConvertError::new("returned object is missing 'state'"))?;
    Ok((result, state))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn json_round_trip_preserves_values() {
        let value = json!({
            "int": 1_i64,
            "float": 1.5,
            "text": "hello",
            "flag": true,
            "none": null,
            "list": [1, "two", null],
            "nested": {"a": 1}
        });
        let monty = json_to_monty(&value, 0).expect("to monty");
        let back = monty_to_json(monty.as_ref(), 0).expect("to json");
        assert_eq!(back, value);
    }

    #[test]
    fn rejects_non_finite_float() {
        let value = Value::Number(
            serde_json::Number::from_f64(f64::INFINITY)
                .unwrap_or_else(|| serde_json::Number::from(0)),
        );
        // serde_json cannot represent infinity, so build the float directly.
        let _ = value;
        let infinite = MontyObject::float(f64::INFINITY);
        assert!(monty_to_json(infinite.as_ref(), 0).is_err());
    }

    #[test]
    fn rejects_non_string_dict_keys() {
        let monty = MontyObject::dict([(MontyObject::int(1), MontyObject::int(2))]);
        assert!(monty_to_json(monty.as_ref(), 0).is_err());
    }

    #[test]
    fn rejects_unsupported_value_type() {
        let monty = MontyObject::complex(1.0, 2.0);
        assert!(monty_to_json(monty.as_ref(), 0).is_err());
    }

    #[test]
    fn completed_requires_state_object() {
        let missing = MontyObject::dict([(MontyObject::string("result"), MontyObject::int(1))]);
        assert!(completed_members(&missing).is_err());

        let invalid = MontyObject::dict([(MontyObject::string("state"), MontyObject::int(1))]);
        assert!(completed_members(&invalid).is_err());
    }

    #[test]
    fn completed_extracts_result_and_state() {
        let value = MontyObject::dict([
            (MontyObject::string("result"), MontyObject::int(7)),
            (
                MontyObject::string("state"),
                MontyObject::dict([(MontyObject::string("count"), MontyObject::int(1))]),
            ),
        ]);
        let (result, state) = completed_members(&value).expect("members");
        assert_eq!(result, json!(7));
        assert_eq!(state, json!({"count": 1}));
    }
}
