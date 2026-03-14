//! Shared WASM-runtime helpers for metadata and registry-related tests.

use std::path::PathBuf;
use std::process::Command;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

use crate::tools::wasm::{ResourceLimits, WasmRuntimeConfig, WasmToolRuntime};

static METADATA_TEST_RUNTIME: OnceLock<Result<Arc<WasmToolRuntime>, String>> = OnceLock::new();

/// Shared WASM runtime for metadata extraction and schema publication regressions.
pub fn metadata_test_runtime() -> anyhow::Result<Arc<WasmToolRuntime>> {
    let runtime = METADATA_TEST_RUNTIME.get_or_init(|| {
        let config = WasmRuntimeConfig {
            default_limits: ResourceLimits::default()
                .with_memory(8 * 1024 * 1024)
                .with_fuel(100_000)
                .with_timeout(Duration::from_secs(5)),
            ..WasmRuntimeConfig::for_testing()
        };
        WasmToolRuntime::new(config)
            .map(Arc::new)
            .map_err(|err| err.to_string())
    });

    match runtime {
        Ok(runtime) => Ok(Arc::clone(runtime)),
        Err(err) => Err(anyhow::anyhow!(err.clone())),
    }
}

/// Source directory for the bundled GitHub WASM test tool.
pub fn github_tool_source_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tools-src/github")
}

/// Locate the GitHub WASM artifact, building it on demand for tests.
pub fn github_wasm_artifact() -> anyhow::Result<PathBuf> {
    static BUILD_LOCK: Mutex<()> = Mutex::new(());

    let source_dir = github_tool_source_dir();
    if let Some(path) =
        crate::registry::artifacts::find_wasm_artifact(&source_dir, "github-tool", "release")
    {
        return Ok(path);
    }

    let _guard = BUILD_LOCK
        .lock()
        .expect("github wasm build lock should not be poisoned");

    if let Some(path) =
        crate::registry::artifacts::find_wasm_artifact(&source_dir, "github-tool", "release")
    {
        return Ok(path);
    }

    let status = Command::new("cargo")
        .arg("build")
        .arg("--manifest-path")
        .arg(source_dir.join("Cargo.toml"))
        .arg("--release")
        .arg("--target")
        .arg("wasm32-wasip2")
        .status()?;

    if !status.success() {
        anyhow::bail!(
            "failed to build GitHub WASM artifact via cargo build (status: {})",
            status
        );
    }

    crate::registry::artifacts::find_wasm_artifact(&source_dir, "github-tool", "release")
        .ok_or_else(|| {
            anyhow::anyhow!(
                "GitHub WASM artifact still missing after build in {}",
                source_dir.display()
            )
        })
}
