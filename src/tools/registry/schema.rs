//! Schema normalisation helpers for WASM tool registration.
//!
//! This module provides functions to normalise and validate parameter schemas
//! during WASM tool registration, handling placeholder schemas, JSON string
//! parsing, and backend-specific format conversions.

/// Parse and validate a schema value stored as a JSON string by text-column backends.
///
/// Returns `None` for empty/null strings and strings that parse to the placeholder
/// schema. Returns the parsed JSON for valid JSON strings, or the trimmed string
/// as a JSON string value for non-JSON input.
pub(super) fn parse_schema_string(s: &str) -> Option<serde_json::Value> {
    use crate::tools::wasm::is_placeholder_schema;

    let trimmed = s.trim();
    if trimmed.is_empty() || trimmed.eq_ignore_ascii_case("null") {
        return None;
    }
    // Attempt to parse JSON strings for backends that return text
    match serde_json::from_str::<serde_json::Value>(trimmed) {
        Ok(parsed) if !is_placeholder_schema(&parsed) => Some(parsed),
        Ok(_) => None,
        Err(_) => Some(serde_json::Value::String(trimmed.to_string())),
    }
}

/// Normalise a schema value for WASM tool registration.
///
/// Converts `Null` values, empty strings, and placeholder schemas to `None`,
/// allowing guest export recovery to run. Parses JSON strings and passes
/// through other values unchanged.
pub(super) fn normalized_schema(schema: serde_json::Value) -> Option<serde_json::Value> {
    use crate::tools::wasm::is_placeholder_schema;

    match schema {
        serde_json::Value::Null => None,
        serde_json::Value::String(value) => parse_schema_string(&value),
        // Treat placeholder schemas as missing so guest export recovery runs.
        value if is_placeholder_schema(&value) => None,
        value => Some(value),
    }
}

#[cfg(test)]
mod tests {
    use rstest::rstest;
    use serde_json::json;

    use super::{normalized_schema, parse_schema_string};

    #[rstest]
    #[case("", None)]
    #[case("   ", None)]
    #[case("null", None)]
    #[case("NULL", None)]
    #[case("NuLl", None)]
    #[case(
        r#"{"type":"object","properties":{},"additionalProperties":true}"#,
        None
    )]
    #[case(
        r#"{"type":"string"}"#,
        Some(json!({"type":"string"}))
    )]
    #[case(
        "not valid json",
        Some(serde_json::Value::String("not valid json".to_string()))
    )]
    fn test_parse_schema_string(#[case] input: &str, #[case] expected: Option<serde_json::Value>) {
        let result = parse_schema_string(input);
        assert_eq!(result, expected);
    }

    #[rstest]
    #[case(serde_json::Value::Null, None)]
    #[case(serde_json::Value::String("".to_string()), None)]
    #[case(
        serde_json::Value::String(
            r#"{"type":"object","properties":{},"additionalProperties":true}"#.to_string()
        ),
        None
    )]
    #[case(
        serde_json::Value::String(r#"{"type":"string"}"#.to_string()),
        Some(json!({"type":"string"}))
    )]
    #[case(
        json!({"type":"object","properties":{},"additionalProperties":true}),
        None
    )]
    #[case(json!({"type":"number"}), Some(json!({"type":"number"})))]
    fn test_normalized_schema(
        #[case] input: serde_json::Value,
        #[case] expected: Option<serde_json::Value>,
    ) {
        let result = normalized_schema(input);
        assert_eq!(result, expected);
    }
}
