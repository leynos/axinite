//! Support modules compiled only for the `tools_and_config` harness.

#[path = "trace_json_patch.rs"]
mod trace_json_patch;
#[path = "trace_test_files.rs"]
pub mod trace_test_files;
#[path = "trace_types.rs"]
pub mod trace_types;

pub mod trace_llm {
    pub(crate) use super::trace_json_patch::patch_json_value;
    pub use super::trace_types::{LlmTrace, TraceExpects};
}

pub(crate) use ironclaw::testing_wasm::{
    github_tool_source_dir, github_wasm_artifact, metadata_test_runtime,
};
