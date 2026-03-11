pub mod assertions;
pub mod cleanup;
pub mod instrumented_llm;
pub mod metrics;
pub mod test_channel;
pub mod test_rig;
pub mod trace_llm;

pub(crate) use ironclaw::testing::{
    github_tool_source_dir, github_wasm_artifact, metadata_test_runtime,
};

const _: fn() -> anyhow::Result<std::sync::Arc<ironclaw::tools::wasm::WasmToolRuntime>> =
    metadata_test_runtime;
const _: fn() -> std::path::PathBuf = github_tool_source_dir;
const _: fn() -> anyhow::Result<std::path::PathBuf> = github_wasm_artifact;
