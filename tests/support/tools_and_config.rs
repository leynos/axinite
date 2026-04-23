//! Support modules compiled only for the `tools_and_config` harness.

#[path = "trace_types.rs"]
mod trace_model;
#[path = "trace_test_files.rs"]
pub mod trace_test_files;

pub mod trace_llm {
    pub use super::trace_model::{LlmTrace, TraceExpects};

    pub(super) fn patch_json_value(value: &mut serde_json::Value, from: &str, to: &str) {
        match value {
            serde_json::Value::String(s) if s.contains(from) => {
                *s = s.replace(from, to);
            }
            serde_json::Value::Array(arr) => {
                for item in arr {
                    patch_json_value(item, from, to);
                }
            }
            serde_json::Value::Object(obj) => {
                for (_, nested) in obj {
                    patch_json_value(nested, from, to);
                }
            }
            _ => {}
        }
    }
}

pub mod trace_types {
    pub use super::trace_model::{TraceTurn, load_trace_with_mutation};
}

pub(crate) use ironclaw::testing_wasm::{
    github_tool_source_dir, github_wasm_artifact, metadata_test_runtime,
};
