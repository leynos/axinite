//! Tests for the WASM extension upgrade flow.

use super::{make_manager_custom_dirs, make_manager_with_temp_dirs};

#[tokio::test]
async fn test_upgrade_no_installed_extensions() {
    let manager = make_manager_with_temp_dirs().expect("manager should be created");
    let result = manager.upgrade(None).await.unwrap();
    assert!(result.results.is_empty());
    assert!(result.message.contains("No WASM extensions installed"));
}

#[tokio::test]
async fn test_upgrade_mcp_server_rejected() {
    let manager = make_manager_with_temp_dirs().expect("manager should be created");
    // MCP servers can't be upgraded via tool_upgrade
    let err = manager.upgrade(Some("some-mcp")).await;
    // It will fail with NotInstalled because there's no MCP server named "some-mcp",
    // but if it were installed, the MCP code path would be rejected.
    assert!(err.is_err());
}

#[tokio::test]
async fn test_upgrade_up_to_date_extension() {
    let dir = tempfile::tempdir().expect("temp dir");
    let channels_dir = dir.path().join("channels");
    ambient_fs::create_dir_all(&channels_dir).unwrap();

    // Write a fake .wasm file and capabilities with current WIT version
    let wasm_path = channels_dir.join("test-channel.wasm");
    ambient_fs::write(&wasm_path, b"\0asm fake").unwrap();

    let cap_path = channels_dir.join("test-channel.capabilities.json");
    let caps = serde_json::json!({
        "type": "channel",
        "name": "test-channel",
        "wit_version": crate::tools::wasm::WIT_CHANNEL_VERSION,
    });
    ambient_fs::write(&cap_path, serde_json::to_string(&caps).unwrap()).unwrap();

    let manager = make_manager_custom_dirs(dir.path().join("tools"), channels_dir);

    let result = manager.upgrade(Some("test-channel")).await.unwrap();
    assert_eq!(result.results.len(), 1);
    assert_eq!(result.results[0].status, "already_up_to_date");
}

#[tokio::test]
async fn test_upgrade_outdated_not_in_registry() {
    let dir = tempfile::tempdir().expect("temp dir");
    let channels_dir = dir.path().join("channels");
    ambient_fs::create_dir_all(&channels_dir).unwrap();

    // Write a fake .wasm file and capabilities with OLD WIT version
    let wasm_path = channels_dir.join("custom-channel.wasm");
    ambient_fs::write(&wasm_path, b"\0asm fake").unwrap();

    let cap_path = channels_dir.join("custom-channel.capabilities.json");
    let caps = serde_json::json!({
        "type": "channel",
        "name": "custom-channel",
        "wit_version": "0.1.0",
    });
    ambient_fs::write(&cap_path, serde_json::to_string(&caps).unwrap()).unwrap();

    let manager = make_manager_custom_dirs(dir.path().join("tools"), channels_dir);

    let result = manager.upgrade(Some("custom-channel")).await.unwrap();
    assert_eq!(result.results.len(), 1);
    assert_eq!(result.results[0].status, "not_in_registry");
}
