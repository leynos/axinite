//! Support modules compiled only for the `tools_and_config` harness.

#[path = "trace_test_files.rs"]
pub mod trace_test_files;
#[path = "trace_types.rs"]
pub mod trace_types;
mod trace_types_builders;
mod trace_types_patch;
mod trace_types_recorded;
mod trace_types_runtime;

/// Narrowed trace-test facade for trace-format tests.
pub mod trace_llm {
    pub use super::trace_types::{LlmTrace, TraceExpects};
}

pub(crate) use ironclaw::testing_wasm::{
    github_tool_source_dir, github_wasm_artifact, metadata_test_runtime,
};
