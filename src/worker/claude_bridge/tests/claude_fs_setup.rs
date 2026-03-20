//! Tests for Claude filesystem setup utilities.

use super::{build_permission_settings, copy_dir_recursive};

#[test]
fn test_build_permission_settings_default_tools() {
    let tools: Vec<String> = ["Bash(*)", "Read", "Edit(*)", "Glob", "Grep"]
        .into_iter()
        .map(String::from)
        .collect();
    let json_str =
        build_permission_settings(&tools).expect("default tool permission settings should build");
    let parsed: serde_json::Value =
        serde_json::from_str(&json_str).expect("settings JSON should parse");
    let allow = parsed["permissions"]["allow"]
        .as_array()
        .expect("allow list should be an array");
    assert_eq!(allow.len(), 5);
    assert_eq!(allow[0], "Bash(*)");
    assert_eq!(allow[1], "Read");
    assert_eq!(allow[2], "Edit(*)");
}

#[test]
fn test_build_permission_settings_empty_tools() {
    let json_str =
        build_permission_settings(&[]).expect("empty tool permission settings should build");
    let parsed: serde_json::Value =
        serde_json::from_str(&json_str).expect("settings JSON should parse");
    let allow = parsed["permissions"]["allow"]
        .as_array()
        .expect("allow list should be an array");
    assert!(allow.is_empty());
}

#[test]
fn test_build_permission_settings_is_valid_json() {
    let tools = vec!["Bash(npm run *)".to_string(), "Read".to_string()];
    let json_str =
        build_permission_settings(&tools).expect("permission settings JSON should build");
    let parsed: serde_json::Value =
        serde_json::from_str(&json_str).expect("settings JSON should parse");
    assert!(parsed["permissions"].is_object());
    assert!(parsed["permissions"]["allow"].is_array());
}

#[test]
fn test_copy_dir_recursive() {
    let src = tempfile::tempdir().expect("create src tempdir");
    let dst = tempfile::tempdir().expect("create dst tempdir");

    std::fs::write(src.path().join("auth.json"), r#"{"token":"abc"}"#).expect("write auth file");
    std::fs::create_dir_all(src.path().join("subdir")).expect("create subdir");
    std::fs::write(src.path().join("subdir").join("nested.txt"), "nested")
        .expect("write nested file");

    let copied = copy_dir_recursive(src.path(), dst.path()).expect("copy directory tree");
    assert_eq!(copied, 2);
    assert_eq!(
        std::fs::read_to_string(dst.path().join("auth.json")).expect("read copied auth file"),
        r#"{"token":"abc"}"#
    );
    assert_eq!(
        std::fs::read_to_string(dst.path().join("subdir").join("nested.txt"))
            .expect("read copied nested file"),
        "nested"
    );
}

#[test]
fn test_copy_dir_recursive_empty_source() {
    let src = tempfile::tempdir().expect("create src tempdir");
    let dst = tempfile::tempdir().expect("create dst tempdir");

    let copied = copy_dir_recursive(src.path(), dst.path()).expect("copy empty directory");
    assert_eq!(copied, 0);
}

#[test]
fn test_copy_dir_recursive_skips_nonexistent_source() {
    let dst = tempfile::tempdir().expect("create dst tempdir");
    let root = tempfile::tempdir().expect("create source root tempdir");
    let nonexistent = root.path().join("no_such_path");

    let copied = copy_dir_recursive(&nonexistent, dst.path()).expect("copy should be graceful");
    assert_eq!(copied, 0);
}
