//! Unit tests for WASM tool discovery from directories and dev build
//! artifacts.

use std::io::Write;

use tempfile::TempDir;

use crate::tools::wasm::loader::dev_tools::{discover_dev_tools, tools_src_dir};
use crate::tools::wasm::loader::discover_tools;

#[tokio::test]
async fn test_discover_tools_empty_dir() {
    let dir = TempDir::new().unwrap();
    let tools = discover_tools(dir.path()).await.unwrap();
    assert!(tools.is_empty());
}

#[tokio::test]
async fn test_discover_tools_with_wasm() {
    let dir = TempDir::new().unwrap();

    // Create a fake .wasm file
    let wasm_path = dir.path().join("test_tool.wasm");
    ambient_fs::File::create(&wasm_path).unwrap();

    let tools = discover_tools(dir.path()).await.unwrap();
    assert_eq!(tools.len(), 1);
    assert!(tools.contains_key("test_tool"));
    assert!(tools["test_tool"].capabilities_path.is_none());
}

#[tokio::test]
async fn test_discover_tools_with_capabilities() {
    let dir = TempDir::new().unwrap();

    // Create wasm and capabilities files
    ambient_fs::File::create(dir.path().join("slack.wasm")).unwrap();
    let mut cap_file =
        ambient_fs::File::create(dir.path().join("slack.capabilities.json")).unwrap();
    cap_file.write_all(b"{}").unwrap();

    let tools = discover_tools(dir.path()).await.unwrap();
    assert_eq!(tools.len(), 1);
    assert!(tools["slack"].capabilities_path.is_some());
}

#[tokio::test]
async fn test_discover_tools_ignores_non_wasm() {
    let dir = TempDir::new().unwrap();

    // Create non-wasm files
    ambient_fs::File::create(dir.path().join("readme.md")).unwrap();
    ambient_fs::File::create(dir.path().join("config.json")).unwrap();
    ambient_fs::File::create(dir.path().join("tool.wasm")).unwrap();

    let tools = discover_tools(dir.path()).await.unwrap();
    assert_eq!(tools.len(), 1);
    assert!(tools.contains_key("tool"));
}

#[test]
fn test_tools_src_dir_default() {
    let dir = tools_src_dir();
    assert!(dir.ends_with("tools-src"));
}

#[tokio::test]
async fn test_discover_dev_tools_finds_build_artifacts() {
    // This test relies on the actual tools-src/ directory in the repo.
    // If build artifacts exist, they should be discovered.
    let tools = discover_dev_tools().await.unwrap();

    // If any tools have been built, they should appear with "-tool" suffix
    for (name, discovered) in &tools {
        assert!(
            name.ends_with("-tool"),
            "Dev tool name should end with -tool: {}",
            name
        );
        assert!(
            discovered.wasm_path.exists(),
            "WASM should exist: {:?}",
            discovered.wasm_path
        );
    }
}

#[tokio::test]
async fn test_discover_skips_dotfiles() {
    let dir = TempDir::new().unwrap();

    // Create a dotfile .wasm and a normal .wasm
    ambient_fs::File::create(dir.path().join(".hidden.wasm")).unwrap();
    ambient_fs::File::create(dir.path().join("visible.wasm")).unwrap();

    let tools = discover_tools(dir.path()).await.unwrap();

    // The current implementation discovers ALL .wasm files including dotfiles.
    // This test documents the current behavior: .hidden.wasm IS discovered
    // with the stem ".hidden". A future hardening pass could add dotfile
    // filtering, at which point this assertion should be updated.
    assert!(
        tools.contains_key("visible"),
        "visible.wasm should be discovered"
    );
    assert!(
        tools.contains_key(".hidden"),
        "dotfile .hidden.wasm is currently discovered (no dotfile filter yet)"
    );
    assert_eq!(tools.len(), 2);
}

#[tokio::test]
async fn test_discover_tools_ignores_subdirectories() {
    let dir = TempDir::new().unwrap();

    // Create a top-level wasm file
    ambient_fs::File::create(dir.path().join("top_level.wasm")).unwrap();

    // Create a subdirectory with a wasm file inside
    let sub_dir = dir.path().join("subdir");
    ambient_fs::create_dir(&sub_dir).unwrap();
    ambient_fs::File::create(sub_dir.join("nested.wasm")).unwrap();

    let tools = discover_tools(dir.path()).await.unwrap();

    // Only top-level files should be discovered (read_dir is not recursive)
    assert_eq!(tools.len(), 1, "Only top-level .wasm files should be found");
    assert!(
        tools.contains_key("top_level"),
        "top_level.wasm should be discovered"
    );
    assert!(
        !tools.contains_key("nested"),
        "nested.wasm inside subdir should NOT be discovered"
    );
}
