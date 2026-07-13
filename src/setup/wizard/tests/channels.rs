//! Tests for WASM channel discovery and installation helpers.

use std::collections::HashSet;

use tempfile::tempdir;

use super::super::channel_catalog::{
    build_channel_options, discover_wasm_channels, install_missing_bundled_channels,
};
use super::super::*;

#[tokio::test]
async fn test_install_missing_bundled_channels_installs_telegram() {
    // WASM artifacts only exist in dev builds (not CI). Skip gracefully
    // rather than fail when the telegram channel hasn't been compiled.
    if !available_channel_names().contains(&"telegram") {
        eprintln!("skipping: telegram WASM artifacts not built");
        return;
    }

    let dir = tempdir().unwrap();
    let installed = HashSet::<String>::new();

    install_missing_bundled_channels(dir.path(), &installed)
        .await
        .unwrap();

    assert!(dir.path().join("telegram.wasm").exists());
    assert!(dir.path().join("telegram.capabilities.json").exists());
}

#[test]
fn test_build_channel_options_includes_available_when_missing() {
    let discovered = Vec::new();
    let options = build_channel_options(&discovered);
    let available = available_channel_names();
    // All available (built) channels should appear
    for name in &available {
        assert!(
            options.contains(&name.to_string()),
            "expected '{}' in options",
            name
        );
    }
}

#[test]
fn test_build_channel_options_dedupes_available() {
    let discovered = vec![(String::from("telegram"), ChannelCapabilitiesFile::default())];
    let options = build_channel_options(&discovered);
    // telegram should appear exactly once despite being both discovered and available
    assert_eq!(
        options.iter().filter(|n| *n == "telegram").count(),
        1,
        "telegram should not be duplicated"
    );
}

#[tokio::test]
async fn test_discover_wasm_channels_empty_dir() {
    let dir = tempdir().unwrap();
    let channels = discover_wasm_channels(dir.path()).await;
    assert!(channels.is_empty());
}

#[tokio::test]
async fn test_discover_wasm_channels_nonexistent_dir() {
    let channels =
        discover_wasm_channels(&std::env::temp_dir().join("ironclaw_nonexistent_dir_abcxyz123"))
            .await;
    assert!(channels.is_empty());
}
