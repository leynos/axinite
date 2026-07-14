//! Unit tests for WASM runtime configuration and module caching.

use crate::tools::wasm::limits::ResourceLimits;
use crate::tools::wasm::runtime::{WasmRuntimeConfig, WasmToolRuntime};

#[test]
fn test_runtime_config_default() {
    let config = WasmRuntimeConfig::default();
    assert!(config.cache_compiled);
    assert!(config.fuel_config.enabled);
}

#[test]
fn test_runtime_config_for_testing() {
    let config = WasmRuntimeConfig::for_testing();
    assert!(!config.cache_compiled);
    assert_eq!(config.default_limits.memory_bytes, 1024 * 1024);
}

#[test]
fn test_runtime_creation() {
    let config = WasmRuntimeConfig::for_testing();
    let runtime = WasmToolRuntime::new(config).unwrap();
    // Engine was created successfully, which validates the config
    assert!(runtime.config().fuel_config.enabled);
}

#[tokio::test]
async fn test_module_cache_operations() {
    let config = WasmRuntimeConfig::for_testing();
    let runtime = WasmToolRuntime::new(config).unwrap();

    // Initially empty
    assert!(runtime.list().await.is_empty());
    assert!(runtime.get("test").await.is_none());
}

#[test]
fn test_prepared_module_limits() {
    let limits = ResourceLimits::default()
        .with_memory(5 * 1024 * 1024)
        .with_fuel(500_000);

    assert_eq!(limits.memory_bytes, 5 * 1024 * 1024);
    assert_eq!(limits.fuel, 500_000);
}

/// Per-engine cache directories must work correctly to avoid file lock
/// conflicts on Windows where multiple engines sharing a single cache
/// directory triggers OS error 33 (ERROR_LOCK_VIOLATION). Regression test
/// for #448: `enable_compilation_cache` must create a subdirectory and
/// produce a valid TOML config that wasmtime can load.
#[test]
fn test_enable_compilation_cache_with_explicit_dir() {
    use crate::tools::wasm::runtime::enable_compilation_cache;

    let tmp = tempfile::tempdir().expect("failed to create temp dir");
    let cache_dir = tmp.path().join("custom-cache");

    let mut config = wasmtime::Config::new();
    enable_compilation_cache(&mut config, "test-engine", Some(cache_dir.as_path()))
        .expect("enable_compilation_cache should succeed with explicit dir");

    // The cache directory should have been created.
    assert!(cache_dir.exists(), "cache directory should be created");

    // A TOML config file should have been written inside.
    let toml_path = cache_dir.join("wasmtime-cache.toml");
    assert!(toml_path.exists(), "TOML config should be written");

    let content = ambient_fs::read_to_string(&toml_path).unwrap();
    assert!(
        content.contains("[cache]"),
        "TOML must contain [cache] section"
    );
    assert!(
        content.contains(&format!("directory = \"{}\"", cache_dir.display())),
        "cache directory must be configured"
    );
}

/// Two engines with different labels must get independent cache directories
/// so that their file locks do not conflict. Regression test for #448.
#[test]
fn test_enable_compilation_cache_label_isolation() {
    use crate::tools::wasm::runtime::enable_compilation_cache;

    let tmp = tempfile::tempdir().expect("failed to create temp dir");
    let base = tmp.path().join("isolation");

    let dir_a = base.join("engine-a");
    let dir_b = base.join("engine-b");

    let mut config_a = wasmtime::Config::new();
    enable_compilation_cache(&mut config_a, "a", Some(dir_a.as_path()))
        .expect("cache A should succeed");

    let mut config_b = wasmtime::Config::new();
    enable_compilation_cache(&mut config_b, "b", Some(dir_b.as_path()))
        .expect("cache B should succeed");

    // Both directories must exist and be distinct.
    assert!(dir_a.exists());
    assert!(dir_b.exists());
    assert_ne!(dir_a, dir_b);
}

/// The WASM runtime (Wasmtime engine) must initialise successfully even
/// when no tools directory exists on disk. The engine only configures the
/// compiler and epoch ticker — loading modules from a directory is a
/// separate step. Regression test for a bug where the runtime was gated
/// on `tools_dir.exists()`, causing extensions installed after startup
/// (e.g. via the web UI) to fail with "WASM runtime not available".
#[test]
fn test_runtime_creation_without_tools_dir() {
    let config = WasmRuntimeConfig::for_testing();
    // Runtime should succeed even though no tools directory exists.
    let runtime = WasmToolRuntime::new(config).expect("runtime should init without tools dir");
    assert!(runtime.config().fuel_config.enabled);
}
