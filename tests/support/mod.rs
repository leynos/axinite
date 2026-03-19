//! Shared test-support utilities used across integration tests.
//!
//! Provides reusable assertions, cleanup helpers, instrumented LLMs, metrics,
//! channels, rigs, and trace helpers.

pub mod assertions;
pub mod cleanup;
pub mod fixtures;
pub mod instrumented_llm;
pub mod metrics;
pub mod telegram;
pub mod test_channel;
pub mod test_rig;
pub mod trace_llm;
mod trace_provider;
pub mod trace_types;

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
const _: fn() -> cleanup::CleanupGuard = cleanup::CleanupGuard::new;
const _: fn(cleanup::CleanupGuard, String) -> cleanup::CleanupGuard = cleanup::CleanupGuard::file;
const _: fn(cleanup::CleanupGuard, String) -> cleanup::CleanupGuard = cleanup::CleanupGuard::dir;
const _: fn(&str) -> std::io::Result<()> = cleanup::setup_test_dir;
const _: fn(&std::path::Path, &str) -> std::io::Result<String> =
    cleanup::setup_test_dir_with_suffix;
