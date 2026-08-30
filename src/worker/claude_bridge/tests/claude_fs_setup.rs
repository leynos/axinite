//! Tests for Claude filesystem setup utilities.

use crate::test_support::ExpectValid;
use rstest::rstest;

use super::{build_permission_settings, copy_dir_recursive};

fn parse_allow_list(tools: &[String]) -> Vec<serde_json::Value> {
    let json_str =
        build_permission_settings(tools).expect_valid("permission settings should build");
    let parsed: serde_json::Value =
        serde_json::from_str(&json_str).expect_valid("settings JSON should parse");
    parsed["permissions"]["allow"]
        .as_array()
        .expect_valid("allow list should be an array")
        .clone()
}

#[rstest]
#[case(
    vec!["Bash(*)".into(), "Read".into(), "Edit(*)".into(), "Glob".into(), "Grep".into()],
    5,
    vec![Some("Bash(*)"), Some("Read"), Some("Edit(*)")],
)]
#[case(vec![], 0, vec![])]
#[case(
    vec!["Bash(npm run *)".into(), "Read".into()],
    2,
    vec![Some("Bash(npm run *)"), Some("Read")],
)]
fn test_build_permission_settings(
    #[case] tools: Vec<String>,
    #[case] expected_len: usize,
    #[case] expected_entries: Vec<Option<&str>>,
) {
    let allow = parse_allow_list(&tools);
    assert_eq!(allow.len(), expected_len);
    for (i, expected) in expected_entries.iter().enumerate() {
        if let Some(val) = expected {
            assert_eq!(allow[i], *val);
        }
    }
}

#[test]
fn test_copy_dir_recursive() {
    let src = tempfile::tempdir().expect_valid("create src tempdir");
    let dst = tempfile::tempdir().expect_valid("create dst tempdir");

    ambient_fs::write(src.path().join("auth.json"), r#"{"token":"abc"}"#)
        .expect_valid("write auth file");
    ambient_fs::create_dir_all(src.path().join("subdir")).expect_valid("create subdir");
    ambient_fs::write(src.path().join("subdir").join("nested.txt"), "nested")
        .expect_valid("write nested file");

    let copied = copy_dir_recursive(src.path(), dst.path()).expect_valid("copy directory tree");
    assert_eq!(copied, 2);
    assert_eq!(
        ambient_fs::read_to_string(dst.path().join("auth.json"))
            .expect_valid("read copied auth file"),
        r#"{"token":"abc"}"#
    );
    assert_eq!(
        ambient_fs::read_to_string(dst.path().join("subdir").join("nested.txt"))
            .expect_valid("read copied nested file"),
        "nested"
    );
}

#[test]
fn test_copy_dir_recursive_empty_source() {
    let src = tempfile::tempdir().expect_valid("create src tempdir");
    let dst = tempfile::tempdir().expect_valid("create dst tempdir");

    let copied = copy_dir_recursive(src.path(), dst.path()).expect_valid("copy empty directory");
    assert_eq!(copied, 0);
}

#[test]
fn test_copy_dir_recursive_skips_nonexistent_source() {
    let dst = tempfile::tempdir().expect_valid("create dst tempdir");
    let root = tempfile::tempdir().expect_valid("create source root tempdir");
    let nonexistent = root.path().join("no_such_path");

    let copied =
        copy_dir_recursive(&nonexistent, dst.path()).expect_valid("copy should be graceful");
    assert_eq!(copied, 0);
}

#[test]
fn test_copy_dir_recursive_propagates_destination_errors() {
    let src = tempfile::tempdir().expect_valid("create src tempdir");
    let dst = tempfile::tempdir().expect_valid("create dst tempdir");

    ambient_fs::create_dir_all(src.path().join("subdir")).expect_valid("create source subdir");
    ambient_fs::write(src.path().join("subdir").join("nested.txt"), "nested")
        .expect_valid("write nested source file");
    ambient_fs::write(dst.path().join("subdir"), "not a directory")
        .expect_valid("block destination subdir path");

    let error = copy_dir_recursive(src.path(), dst.path())
        .expect_err("destination-side failures should be returned");
    assert_ne!(
        error.kind(),
        std::io::ErrorKind::NotFound,
        "destination errors should not be downgraded to missing source"
    );
}
