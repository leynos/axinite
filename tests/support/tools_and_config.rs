//! Shared support for tool-schema and trace-format tests.

pub mod trace_types;
mod trace_types_recorded;

pub use ironclaw::testing_wasm::{
    github_tool_source_dir, github_wasm_artifact, metadata_test_runtime,
};
