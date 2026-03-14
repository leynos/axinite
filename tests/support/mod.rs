//! Shared test-support utilities used across integration tests.
//!
//! Provides reusable assertions, cleanup helpers, instrumented LLMs, metrics,
//! channels, rigs, and trace helpers.

pub mod assertions;
pub mod cleanup;
pub mod instrumented_llm;
pub mod metrics;
pub mod telegram;
pub mod test_channel;
pub mod test_rig;
pub mod trace_llm;

pub(crate) use ironclaw::testing_wasm::{
    github_tool_source_dir, github_wasm_artifact, metadata_test_runtime,
};

// These function-pointer constants intentionally perform compile-time type
// assertions. They catch signature mismatches for shared test helpers during
// compilation and have no runtime effect.
const _: fn() -> anyhow::Result<std::sync::Arc<ironclaw::tools::wasm::WasmToolRuntime>> =
    metadata_test_runtime;
const _: fn() -> std::path::PathBuf = github_tool_source_dir;
const _: fn() -> anyhow::Result<std::path::PathBuf> = github_wasm_artifact;
