//! Shared test fixtures and builders for complex tool definitions.
//!
//! This module provides canonical complex tool definition builders that are
//! reused across both orchestrator and worker test suites to ensure consistency
//! and reduce duplication.

use crate::llm::ToolDefinition;
use serde::Serialize;
use serde::de::DeserializeOwned;
use std::fmt::{Debug, Display};

/// A named panic boundary for test-only helpers without an error channel.
///
/// Whitaker treats those helpers as non-test code, so direct `expect` calls
/// would hide a fallible boundary from the caller. Tests retain the same
/// diagnostics while recording this intentional assertion boundary explicitly.
pub(crate) trait ExpectValid {
    /// The value returned when the assertion succeeds.
    type Value;

    /// Returns the successful value or fails at the calling test location.
    #[track_caller]
    fn expect_valid(self, message: &str) -> Self::Value;
}

impl<T, E: Display> ExpectValid for Result<T, E> {
    type Value = T;

    fn expect_valid(self, message: &str) -> Self::Value {
        match self {
            Ok(value) => value,
            Err(error) => panic!("{message}: {error}"),
        }
    }
}

impl<T> ExpectValid for Option<T> {
    type Value = T;

    fn expect_valid(self, message: &str) -> Self::Value {
        match self {
            Some(value) => value,
            None => panic!("{message}"),
        }
    }
}

/// Serialises `value` to JSON, deserialises it back and returns the copy.
///
/// Panics at the calling test location if either direction fails.
#[track_caller]
fn json_round_trip<T>(value: &T) -> (String, T)
where
    T: Serialize + DeserializeOwned,
{
    let json = match serde_json::to_string(value) {
        Ok(json) => json,
        Err(error) => panic!("serialise round-trip value: {error}"),
    };
    match serde_json::from_str::<T>(&json) {
        Ok(back) => (json, back),
        Err(error) => panic!("deserialise round-trip value from {json}: {error}"),
    }
}

/// Asserts that `value` survives a JSON serialise/deserialise cycle unchanged.
///
/// Use this for types that derive `PartialEq`. Callers that also pin the wire
/// format should keep their own assertion on the serialised string.
#[track_caller]
pub(crate) fn assert_json_round_trip<T>(value: &T)
where
    T: Serialize + DeserializeOwned + PartialEq + Debug,
{
    let (json, back) = json_round_trip(value);
    assert_eq!(
        &back, value,
        "value must round-trip without field loss (json: {json})"
    );
}

/// Like [`assert_json_round_trip`], but compares with `is_same` instead of
/// `PartialEq`, for types that deliberately do not implement equality.
///
/// A closure is used (rather than comparing re-serialised JSON) so that the
/// comparison covers the deserialised fields themselves, and a field that
/// fails to deserialise cannot hide behind an identical default encoding.
#[track_caller]
pub(crate) fn assert_json_round_trip_with<T>(value: &T, is_same: impl FnOnce(&T, &T) -> bool)
where
    T: Serialize + DeserializeOwned + Debug,
{
    let (json, back) = json_round_trip(value);
    assert!(
        is_same(value, &back),
        "value must round-trip without field loss (json: {json}); original: {value:?}, \
         deserialised: {back:?}"
    );
}

/// Returns the canonical complex parameters JSON schema used for fidelity testing.
///
/// This schema exercises nested objects, arrays, enums, constraints, and various
/// JSON Schema features to validate that tool definitions survive serialization,
/// transport, and reconstruction without data loss.
pub fn complex_tool_definition_parameters() -> serde_json::Value {
    serde_json::json!({
        "type": "object",
        "title": "ComplexParams",
        "description": "Nested schema with multiple property types",
        "properties": {
            "query": {
                "type": "string",
                "description": "Search query with constraints",
                "minLength": 1,
                "maxLength": 500
            },
            "options": {
                "type": "object",
                "description": "Nested configuration object",
                "properties": {
                    "limit": {
                        "type": "integer",
                        "minimum": 1,
                        "maximum": 100,
                        "default": 10
                    },
                    "include_metadata": {
                        "type": "boolean",
                        "default": false
                    },
                    "filters": {
                        "type": "array",
                        "items": {
                            "type": "string",
                            "enum": ["active", "archived", "draft"]
                        }
                    }
                },
                "required": ["limit"]
            },
            "callback_url": {
                "type": "string",
                "format": "uri",
                "description": "Optional webhook URL"
            }
        },
        "required": ["query", "options"],
        "additionalProperties": false
    })
}

/// Builds a complex tool definition for fidelity testing.
///
/// Constructs a `ToolDefinition` with the given name and description, using
/// the canonical complex parameters schema. This ensures both orchestrator and
/// worker tests use identical parameter structures while allowing test-specific
/// names and descriptions.
///
/// # Arguments
///
/// * `name` - The tool name (e.g., "remote_tool_fidelity_fixture")
/// * `description` - The tool description (should include complex UTF-8, markdown, etc.)
///
/// # Returns
///
/// A `ToolDefinition` with the canonical complex parameters schema.
pub fn build_complex_tool_definition(name: &str, description: &str) -> ToolDefinition {
    ToolDefinition {
        name: name.to_string(),
        description: description.to_string(),
        parameters: complex_tool_definition_parameters(),
    }
}
