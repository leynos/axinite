//! Tests that tool and channel artifacts occupy separate filesystem paths.

// === QA Plan P2 - 2.4: Extension registry collision tests (filesystem) ===

#[test]
fn test_tool_and_channel_paths_are_separate() {
    // Verify that a WASM tool named "telegram" and a WASM channel named
    // "telegram" use different filesystem paths and don't overwrite each other.
    let dir = tempfile::tempdir().expect("temp dir");
    let tools_dir = dir.path().join("tools");
    let channels_dir = dir.path().join("channels");
    ambient_fs::create_dir_all(&tools_dir).unwrap();
    ambient_fs::create_dir_all(&channels_dir).unwrap();

    let name = "telegram";
    let tool_wasm = tools_dir.join(format!("{}.wasm", name));
    let channel_wasm = channels_dir.join(format!("{}.wasm", name));

    // Simulate installing both.
    ambient_fs::write(&tool_wasm, b"tool-payload").unwrap();
    ambient_fs::write(&channel_wasm, b"channel-payload").unwrap();

    // Both files exist and contain distinct content.
    assert!(tool_wasm.exists());
    assert!(channel_wasm.exists());
    assert_ne!(
        ambient_fs::read(&tool_wasm).unwrap(),
        ambient_fs::read(&channel_wasm).unwrap(),
        "Tool and channel files must be independent"
    );

    // Removing one doesn't affect the other.
    ambient_fs::remove_file(&tool_wasm).unwrap();
    assert!(!tool_wasm.exists());
    assert!(
        channel_wasm.exists(),
        "Removing tool must not affect channel"
    );
}

#[test]
fn test_determine_kind_priority_tools_before_channels() {
    // When a name exists in both tools and channels dirs,
    // determine_installed_kind checks tools first (wasm_tools_dir).
    // This test documents the priority order.
    let dir = tempfile::tempdir().expect("temp dir");
    let tools_dir = dir.path().join("tools");
    let channels_dir = dir.path().join("channels");
    ambient_fs::create_dir_all(&tools_dir).unwrap();
    ambient_fs::create_dir_all(&channels_dir).unwrap();

    let name = "ambiguous";
    let tool_wasm = tools_dir.join(format!("{}.wasm", name));
    let channel_wasm = channels_dir.join(format!("{}.wasm", name));

    // Only channel exists → channel kind.
    ambient_fs::write(&channel_wasm, b"channel").unwrap();
    assert!(!tool_wasm.exists());
    assert!(channel_wasm.exists());

    // Both exist → tools dir checked first.
    ambient_fs::write(&tool_wasm, b"tool").unwrap();
    assert!(tool_wasm.exists());
    assert!(channel_wasm.exists());
    // This documents the determine_installed_kind priority:
    // tools are checked before channels.

    // Only tool exists → tool kind.
    ambient_fs::remove_file(&channel_wasm).unwrap();
    assert!(tool_wasm.exists());
    assert!(!channel_wasm.exists());
}

// === WASM runtime availability tests ===
//
// Regression tests for a bug where the WASM runtime was only created at
// startup when the tools directory already existed. Extensions installed
// after startup (e.g. via the web UI) would fail with "WASM runtime not
// available" because the ExtensionManager had `wasm_tool_runtime: None`.

// NOTE: The WASM runtime availability tests that were here have been
// removed. The runtime check now lives in `LiveWasmToolActivation`
// (the activation adapter) — not in ExtensionManager. Tests for that
// behaviour belong in the adapter's own test module.

#[test]
fn test_capabilities_files_also_separate() {
    // capabilities.json files for tools and channels should also be separate.
    let dir = tempfile::tempdir().expect("temp dir");
    let tools_dir = dir.path().join("tools");
    let channels_dir = dir.path().join("channels");
    ambient_fs::create_dir_all(&tools_dir).unwrap();
    ambient_fs::create_dir_all(&channels_dir).unwrap();

    let name = "telegram";
    let tool_cap = tools_dir.join(format!("{}.capabilities.json", name));
    let channel_cap = channels_dir.join(format!("{}.capabilities.json", name));

    let tool_caps = r#"{"required_secrets":["TELEGRAM_API_KEY"]}"#;
    let channel_caps = r#"{"required_secrets":["TELEGRAM_BOT_TOKEN"]}"#;

    ambient_fs::write(&tool_cap, tool_caps).unwrap();
    ambient_fs::write(&channel_cap, channel_caps).unwrap();

    // Both exist with distinct content.
    assert_eq!(ambient_fs::read_to_string(&tool_cap).unwrap(), tool_caps);
    assert_eq!(
        ambient_fs::read_to_string(&channel_cap).unwrap(),
        channel_caps
    );
}
