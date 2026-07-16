//! Loading and security regression tests for the WASM tool loader: name
//! validation, missing/invalid inputs, and guest schema publication.

use std::io::Write;
use std::sync::Arc;

use anyhow::Context as _;
use tempfile::TempDir;

use crate::llm::ToolDefinition;
use crate::testing::{github_tool_source_dir, github_wasm_artifact, metadata_test_runtime};
use crate::tools::registry::ToolRegistry;
use crate::tools::wasm::loader::dev_tools::load_dev_tools;
use crate::tools::wasm::loader::{WasmLoadError, WasmToolLoader};
use crate::tools::wasm::{WasmRuntimeConfig, WasmToolRuntime};

#[test]
fn test_load_error_display() {
    let err = WasmLoadError::InvalidName("bad/name".to_string());
    assert!(err.to_string().contains("bad/name"));

    let err = WasmLoadError::WasmNotFound(std::path::PathBuf::from("/foo/bar.wasm"));
    assert!(err.to_string().contains("/foo/bar.wasm"));
}

/// Helper: create a WasmToolLoader backed by a real runtime + registry.
fn make_loader() -> anyhow::Result<WasmToolLoader> {
    let runtime = Arc::new(
        WasmToolRuntime::new(WasmRuntimeConfig::for_testing())
            .context("failed to create WASM runtime for test")?,
    );
    let registry = Arc::new(ToolRegistry::new());
    Ok(WasmToolLoader::new(runtime, registry))
}

fn make_metadata_loader() -> anyhow::Result<(WasmToolLoader, Arc<ToolRegistry>)> {
    let runtime = metadata_test_runtime().context("create metadata test runtime")?;
    let registry = Arc::new(ToolRegistry::new());
    Ok((
        WasmToolLoader::new(runtime, Arc::clone(&registry)),
        registry,
    ))
}

fn assert_real_github_schema(definition: ToolDefinition) {
    crate::testing::github::assert_real_github_schema(&definition.parameters);
}

#[tokio::test]
async fn test_tool_name_rejects_path_separators() {
    let dir = TempDir::new().unwrap();
    // Create a valid wasm file so the name check is the only failure path
    let wasm_path = dir.path().join("dummy.wasm");
    ambient_fs::File::create(&wasm_path).unwrap();

    let loader = make_loader().expect("failed to create WASM loader for test");

    for bad_name in &["../evil", "foo/bar", "foo\\bar"] {
        let result = loader.load_from_files(bad_name, &wasm_path, None).await;
        assert!(
            result.is_err(),
            "Expected error for name {:?}, got Ok",
            bad_name
        );
        let err = result.unwrap_err();
        assert!(
            matches!(err, WasmLoadError::InvalidName(_)),
            "Expected InvalidName for {:?}, got: {}",
            bad_name,
            err
        );
    }
}

#[tokio::test]
async fn test_tool_name_rejects_empty() {
    let dir = TempDir::new().unwrap();
    let wasm_path = dir.path().join("dummy.wasm");
    ambient_fs::File::create(&wasm_path).unwrap();

    let loader = make_loader().expect("failed to create WASM loader for test");
    let result = loader.load_from_files("", &wasm_path, None).await;

    assert!(result.is_err(), "Expected error for empty name, got Ok");
    let err = result.unwrap_err();
    assert!(
        matches!(err, WasmLoadError::InvalidName(_)),
        "Expected InvalidName for empty string, got: {}",
        err
    );
}

#[tokio::test]
async fn test_load_nonexistent_wasm_file() {
    let loader = make_loader().expect("failed to create WASM loader for test");
    let bogus_path = std::path::PathBuf::from("/tmp/nonexistent_tool_12345.wasm");

    let result = loader.load_from_files("bogus", &bogus_path, None).await;
    assert!(
        result.is_err(),
        "Expected error for nonexistent file, got Ok"
    );
    let err = result.unwrap_err();
    assert!(
        matches!(err, WasmLoadError::WasmNotFound(_)),
        "Expected WasmNotFound, got: {}",
        err
    );
}

#[tokio::test]
async fn test_load_invalid_wasm_bytes() {
    let dir = TempDir::new().unwrap();
    let wasm_path = dir.path().join("invalid.wasm");

    // Write random invalid bytes (not a valid WASM module)
    let mut f = ambient_fs::File::create(&wasm_path).unwrap();
    f.write_all(b"this is not a valid wasm module at all")
        .unwrap();

    let loader = make_loader().expect("failed to create WASM loader for test");
    let result = loader.load_from_files("invalid", &wasm_path, None).await;

    assert!(
        result.is_err(),
        "Expected error for invalid WASM bytes, got Ok"
    );
    // The error should come from WASM compilation or registration, not name validation
    let err = result.unwrap_err();
    assert!(
        !matches!(err, WasmLoadError::InvalidName(_)),
        "Got InvalidName instead of a compilation/registration error: {}",
        err
    );
}

#[tokio::test]
async fn load_from_dir_returns_empty_when_dir_missing() {
    let loader = make_loader().expect("failed to create WASM loader for test");

    let dir = TempDir::new().unwrap();
    let missing = dir.path().join("nonexistent_tools_dir");

    let results = loader.load_from_dir(&missing).await;

    // Must succeed with empty results, not error
    let results = results.expect("missing dir should return Ok, not Err");
    assert!(results.loaded.is_empty());
    assert!(results.errors.is_empty());
}

#[tokio::test]
async fn load_from_files_publishes_guest_schema_in_tool_definitions() {
    let wasm_path = github_wasm_artifact().expect("build or find github WASM artefact");
    let capabilities_path = github_tool_source_dir().join("github-tool.capabilities.json");
    let (loader, registry) =
        make_metadata_loader().expect("failed to create metadata loader for test");

    loader
        .load_from_files("github", &wasm_path, Some(&capabilities_path))
        .await
        .expect("load github wasm tool from file");

    assert_real_github_schema(
        registry
            .tool_definitions()
            .await
            .into_iter()
            .find(|definition| definition.name == "github")
            .expect("github definition should be registered"),
    );
}

#[tokio::test]
async fn load_dev_tools_publishes_guest_schema_in_tool_definitions() {
    github_wasm_artifact().expect("build or find github WASM artefact");
    let install_dir = TempDir::new().expect("create install dir");
    let (loader, registry) =
        make_metadata_loader().expect("failed to create metadata loader for test");

    let results = load_dev_tools(&loader, install_dir.path())
        .await
        .expect("load dev tools");

    assert!(
        results.loaded.iter().any(|name| name == "github-tool"),
        "expected github-tool to load from dev artefacts: {:?}",
        results.loaded
    );

    assert_real_github_schema(
        registry
            .tool_definitions()
            .await
            .into_iter()
            .find(|definition| definition.name == "github-tool")
            .expect("github-tool definition should be registered"),
    );
}
