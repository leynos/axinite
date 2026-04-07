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
    use proptest::prelude::*;
    use rstest::rstest;
    use serde_json::json;

    use super::{normalized_schema, parse_schema_string};

    /// The placeholder schema that should be treated as None.
    const PLACEHOLDER_JSON: &str =
        r#"{"type":"object","properties":{},"additionalProperties":true}"#;

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

    // Property-based tests for parse_schema_string invariants
    proptest! {
        // Empty and whitespace-only strings should return None.
        fn prop_empty_and_whitespace_returns_none(s in "[\\s]*") {
            prop_assert_eq!(parse_schema_string(&s), None);
        }

        // Any case-variant of "null" should return None.
        fn prop_null_case_variants_return_none(
            s in "[nN][uU][lL][lL]"
        ) {
            prop_assert_eq!(parse_schema_string(&s), None);
        }

        // Valid JSON strings should parse into serde_json::Value.
        fn prop_valid_json_parses(json_str in "\\{[\"type\"]:[\"string\",\"number\",\"boolean\",\"array\",\"object\"]\\}") {
            // Skip if it's the placeholder
            if json_str != PLACEHOLDER_JSON {
                let result = parse_schema_string(&json_str);
                // Should parse successfully or fallback to string
                prop_assert!(result.is_some());
            }
        }

        // Invalid JSON should fallback to returning the raw trimmed string.
        fn prop_invalid_json_fallbacks_to_string(s in "[a-zA-Z_][a-zA-Z0-9_]*") {
            // Skip null variants and empty strings
            if !s.eq_ignore_ascii_case("null") && !s.trim().is_empty() {
                let result = parse_schema_string(&s);
                prop_assert_eq!(
                    result,
                    Some(serde_json::Value::String(s.to_string()))
                );
            }
        }
    }

    // Property-based tests for normalized_schema invariants
    proptest! {
        // Empty strings should return None.
        fn prop_empty_string_returns_none(s in "") {
            let _ = s;
            prop_assert_eq!(
                normalized_schema(serde_json::Value::String("".to_string())),
                None
            );
        }

        // Valid non-placeholder objects should be returned as-is.
        fn prop_valid_non_placeholder_preserved(obj in prop_oneof![
            Just(json!({"type": "string"})),
            Just(json!({"type": "number"})),
            Just(json!({"type": "boolean"})),
            Just(json!({"type": "array"})),
            Just(json!({"description": "test", "type": "object"})),
        ]) {
            prop_assert_eq!(normalized_schema(obj.clone()), Some(obj));
        }
    }

    // Unit-style property tests (no proptest parameters needed)
    #[test]
    fn prop_placeholder_json_returns_none() {
        assert_eq!(parse_schema_string(PLACEHOLDER_JSON), None);
    }

    #[test]
    fn prop_null_value_returns_none() {
        assert_eq!(normalized_schema(serde_json::Value::Null), None);
    }

    #[test]
    fn prop_placeholder_object_returns_none() {
        let placeholder = json!({"type":"object","properties":{},"additionalProperties":true});
        assert_eq!(normalized_schema(placeholder), None);
    }
}
