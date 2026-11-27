//! Conversion between the bounded JSON wire values and Monty values.
//!
//! Only JSON null, booleans, exact 64-bit integers, finite binary64 floats,
//! strings, arrays and string-keyed objects cross the boundary. Integers are
//! exact across the whole `i64::MIN ..= u64::MAX` range; a Python `int` wider
//! than that is refused rather than rounded. Everything else is rejected
//! rather than coerced.

use std::fmt;

use monty_protocol::MAX_JSON_DEPTH;
use monty_types::{CallArgs, MontyObject, ObjectRef};
use num_bigint::BigInt;
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
    check_depth(depth)?;
    match value {
        Value::Null => Ok(MontyObject::none()),
        Value::Bool(value) => Ok(MontyObject::bool(*value)),
        Value::Number(number) => number_to_monty(number),
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

/// Refuse a value nested deeper than the JSON ABI allows.
fn check_depth(depth: usize) -> Result<(), ConvertError> {
    if depth > MAX_JSON_DEPTH {
        return Err(ConvertError::new("value exceeds the JSON nesting limit"));
    }
    Ok(())
}

/// Convert a JSON number: an exact 64-bit integer, a finite float, or refused.
///
/// The boundary carries exact signed *and* unsigned 64-bit integers. An
/// unsigned value above `i64::MAX` does not fit Monty's `Int` variant, so it
/// becomes a `BigInt`, which is how Python represents it. Anything wider is
/// refused rather than rounded.
fn number_to_monty(number: &serde_json::Number) -> Result<MontyObject, ConvertError> {
    if let Some(integer) = number.as_i64() {
        return Ok(MontyObject::int(integer));
    }
    if let Some(unsigned) = number.as_u64() {
        return Ok(MontyObject::bigint(BigInt::from(unsigned)));
    }
    let Some(float) = number.as_f64().filter(|value| value.is_finite()) else {
        return Err(ConvertError::new(
            "number is outside the exact 64-bit integer range, or is not finite",
        ));
    };
    Ok(MontyObject::float(float))
}

/// Converts a Monty value into a JSON value.
pub fn monty_to_json(value: ObjectRef<'_>, depth: usize) -> Result<Value, ConvertError> {
    check_depth(depth)?;
    if let Some(scalar) = monty_scalar_to_json(value)? {
        return Ok(scalar);
    }
    match value.type_name() {
        "list" | "tuple" => monty_list_to_json(value, depth),
        "dict" => monty_dict_to_json(value, depth),
        other => Err(ConvertError::new(format!(
            "value of type '{other}' has no JSON representation"
        ))),
    }
}

/// Convert a Monty scalar, or return `None` when the value is a container.
fn monty_scalar_to_json(value: ObjectRef<'_>) -> Result<Option<Value>, ConvertError> {
    if value == MontyObject::none() {
        return Ok(Some(Value::Null));
    }
    if let Some(boolean) = value.as_bool() {
        return Ok(Some(Value::Bool(boolean)));
    }
    // Checked by type name, not by `as_int`: a Python `int` that does not fit
    // `i64` would otherwise fall through to `as_float`, which narrows a
    // `BigInt` to `f64` and returns a rounded value as if it were exact.
    if value.type_name() == "int" {
        return exact_integer_to_json(value).map(Some);
    }
    if let Some(float) = value.as_float() {
        let number = serde_json::Number::from_f64(float)
            .ok_or_else(|| ConvertError::new("float is not finite"))?;
        return Ok(Some(Value::Number(number)));
    }
    Ok(value.as_str().map(|text| Value::String(text.to_owned())))
}

/// Convert a Python `int` to an exact JSON number, or refuse it.
///
/// `int` is arbitrary precision in Python, while the boundary carries exactly
/// 64 bits. `Display` renders both Monty representations as exact decimal
/// digits, so parsing that text against the two 64-bit ranges is lossless in
/// both directions. A value outside `i64::MIN ..= u64::MAX` is refused rather
/// than rounded.
fn exact_integer_to_json(value: ObjectRef<'_>) -> Result<Value, ConvertError> {
    let text = value.to_string();
    if let Ok(unsigned) = text.parse::<u64>() {
        return Ok(Value::Number(unsigned.into()));
    }
    if let Ok(signed) = text.parse::<i64>() {
        return Ok(Value::Number(signed.into()));
    }
    Err(ConvertError::new(
        "integer is outside the exact signed/unsigned 64-bit range",
    ))
}

/// Convert a Monty list or tuple to a JSON array.
fn monty_list_to_json(value: ObjectRef<'_>, depth: usize) -> Result<Value, ConvertError> {
    let items = value
        .items()
        .ok_or_else(|| ConvertError::new("list value has no items"))?;
    let items = items
        .into_iter()
        .map(|item| monty_to_json(item, depth + 1))
        .collect::<Result<Vec<_>, _>>()?;
    Ok(Value::Array(items))
}

/// Convert a Monty dict to a JSON object, requiring string keys.
fn monty_dict_to_json(value: ObjectRef<'_>, depth: usize) -> Result<Value, ConvertError> {
    let pairs = value
        .pairs()
        .ok_or_else(|| ConvertError::new("dict value has no items"))?;
    let mut object = Map::new();
    for (key, item) in pairs {
        let key = key
            .as_str()
            .ok_or_else(|| ConvertError::new("object keys must be strings in the JSON ABI"))?;
        object.insert(key.to_owned(), monty_to_json(item, depth + 1)?);
    }
    Ok(Value::Object(object))
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

    /// The boundary carries the whole `i64::MIN ..= u64::MAX` range exactly:
    /// an unsigned value above `i64::MAX` is a Python `int` too.
    #[test]
    fn preserves_unsigned_integers_above_i64_max() {
        let value = json!(u64::MAX);
        let monty = json_to_monty(&value, 0).expect("to monty");
        let back = monty_to_json(monty.as_ref(), 0).expect("to json");
        assert_eq!(back, value);
        assert_eq!(back.as_u64(), Some(u64::MAX));
    }

    #[test]
    fn preserves_the_signed_and_unsigned_extremes() {
        let values = [
            json!(i64::MIN),
            json!(i64::MAX),
            // The first value only an unsigned 64-bit parse accepts.
            json!(9_223_372_036_854_775_808_u64),
            json!(u64::MAX),
        ];
        for value in values {
            let monty = json_to_monty(&value, 0).expect("to monty");
            let back = monty_to_json(monty.as_ref(), 0).expect("to json");
            assert_eq!(back, value);
        }
    }

    /// A Python `int` wider than 64 bits has no exact JSON form here, so it is
    /// refused. It must not be narrowed to a float, which would return a
    /// rounded value as if it were exact.
    #[test]
    fn refuses_an_integer_wider_than_64_bits_rather_than_rounding() {
        let too_wide = MontyObject::bigint(BigInt::from(u64::MAX) + 1);
        assert!(monty_to_json(too_wide.as_ref(), 0).is_err());

        let very_negative = MontyObject::bigint(BigInt::from(i64::MIN) - 1);
        assert!(monty_to_json(very_negative.as_ref(), 0).is_err());
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
