//! Property-based tests for thread message rebuilding logic.
//!
//! These tests use proptest to verify that any `tool_calls` array containing
//! entries with blank/null/non-string `name` or `call_id` fields causes the
//! entire row to be skipped.

use super::*;
use crate::config::SafetyConfig;
use crate::safety::SafetyLayer;
use proptest::prelude::*;

/// Generates a `serde_json::Value` for a single tool-call entry
/// where `name` is blank, null, or a non-string type.
fn bad_name_strategy() -> impl Strategy<Value = serde_json::Value> {
    prop_oneof![
        Just(serde_json::Value::Null),
        Just(serde_json::Value::Bool(false)),
        Just(serde_json::Value::from(0i64)),
        "[ \t]*".prop_map(serde_json::Value::String), // blank/whitespace
    ]
    .prop_map(|bad_name| {
        serde_json::json!({
            "name": bad_name,
            "call_id": "call_0",
            "parameters": {},
            "result": "ok"
        })
    })
}

/// Generates a `serde_json::Value` for a single tool-call entry
/// where `call_id` is blank, null, or a non-string type.
fn bad_call_id_strategy() -> impl Strategy<Value = serde_json::Value> {
    prop_oneof![
        Just(serde_json::Value::Null),
        Just(serde_json::Value::Bool(true)),
        Just(serde_json::Value::from(42i64)),
        "[ \t]*".prop_map(serde_json::Value::String),
    ]
    .prop_map(|bad_id| {
        serde_json::json!({
            "name": "search",
            "call_id": bad_id,
            "parameters": {},
            "result": "ok"
        })
    })
}

proptest! {
    #[test]
    fn prop_bad_name_entry_skips_row(bad_entry in bad_name_strategy()) {
        let safety = SafetyLayer::new(&SafetyConfig {
            injection_check_enabled: false,
            max_output_length: 100_000,
        });
        let arr = serde_json::Value::Array(vec![bad_entry]);
        assert_malformed_tool_calls_skipped(&safety, arr);
    }

    #[test]
    fn prop_bad_call_id_entry_skips_row(bad_entry in bad_call_id_strategy()) {
        let safety = SafetyLayer::new(&SafetyConfig {
            injection_check_enabled: false,
            max_output_length: 100_000,
        });
        let arr = serde_json::Value::Array(vec![bad_entry]);
        assert_malformed_tool_calls_skipped(&safety, arr);
    }
}
