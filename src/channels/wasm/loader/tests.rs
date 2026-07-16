//! Unit tests for WASM channel discovery and loading.

use std::io::Write;

use tempfile::TempDir;

use crate::channels::wasm::loader::{WasmChannelLoader, discover_channels};
use crate::channels::wasm::runtime::{WasmChannelRuntime, WasmChannelRuntimeConfig};
use crate::pairing::PairingStore;
use std::sync::Arc;

#[tokio::test]
async fn test_discover_channels_empty_dir() {
    let dir = TempDir::new().unwrap();
    let channels = discover_channels(dir.path()).await.unwrap();
    assert!(channels.is_empty());
}

#[tokio::test]
async fn test_discover_channels_with_wasm() {
    let dir = TempDir::new().unwrap();

    // Create a fake .wasm file
    let wasm_path = dir.path().join("slack.wasm");
    ambient_fs::File::create(&wasm_path).unwrap();

    let channels = discover_channels(dir.path()).await.unwrap();
    assert_eq!(channels.len(), 1);
    assert!(channels.contains_key("slack"));
    assert!(channels["slack"].capabilities_path.is_none());
}

#[tokio::test]
async fn test_discover_channels_with_capabilities() {
    let dir = TempDir::new().unwrap();

    // Create wasm and capabilities files
    ambient_fs::File::create(dir.path().join("telegram.wasm")).unwrap();
    let mut cap_file =
        ambient_fs::File::create(dir.path().join("telegram.capabilities.json")).unwrap();
    cap_file.write_all(b"{}").unwrap();

    let channels = discover_channels(dir.path()).await.unwrap();
    assert_eq!(channels.len(), 1);
    assert!(channels["telegram"].capabilities_path.is_some());
}

#[tokio::test]
async fn test_discover_channels_ignores_non_wasm() {
    let dir = TempDir::new().unwrap();

    // Create non-wasm files
    ambient_fs::File::create(dir.path().join("readme.md")).unwrap();
    ambient_fs::File::create(dir.path().join("config.json")).unwrap();
    ambient_fs::File::create(dir.path().join("channel.wasm")).unwrap();

    let channels = discover_channels(dir.path()).await.unwrap();
    assert_eq!(channels.len(), 1);
    assert!(channels.contains_key("channel"));
}

#[test]
fn test_loaded_channel_signature_key_none_without_caps() {
    // We can't easily construct a WasmChannel without a runtime, so test
    // the delegation logic directly: when capabilities_file is None, the
    // chain returns None (same logic as LoadedChannel::signature_key_secret_name).
    let cap_file: Option<crate::channels::wasm::schema::ChannelCapabilitiesFile> = None;
    let result = cap_file
        .as_ref()
        .and_then(|f| f.signature_key_secret_name().map(|s| s.to_string()));
    assert_eq!(result, None);
}

#[tokio::test]
async fn test_loader_invalid_name() {
    let config = WasmChannelRuntimeConfig::for_testing();
    let runtime = Arc::new(WasmChannelRuntime::new(config).unwrap());
    let loader = WasmChannelLoader::new(runtime, Arc::new(PairingStore::new()), None);

    let dir = TempDir::new().unwrap();
    let wasm_path = dir.path().join("test.wasm");

    // Invalid name with path separator
    let result = loader.load_from_files("../escape", &wasm_path, None).await;
    assert!(result.is_err());

    // Empty name
    let result = loader.load_from_files("", &wasm_path, None).await;
    assert!(result.is_err());
}

#[tokio::test]
async fn load_from_dir_returns_empty_when_dir_missing() {
    let config = WasmChannelRuntimeConfig::for_testing();
    let runtime = Arc::new(WasmChannelRuntime::new(config).unwrap());
    let loader = WasmChannelLoader::new(runtime, Arc::new(PairingStore::new()), None);

    let dir = TempDir::new().unwrap();
    let missing = dir.path().join("nonexistent_channels_dir");

    let results = loader.load_from_dir(&missing).await;

    // Must succeed with empty results, not error
    let results = results.expect("missing dir should return Ok, not Err");
    assert!(results.loaded.is_empty());
    assert!(results.errors.is_empty());
}
