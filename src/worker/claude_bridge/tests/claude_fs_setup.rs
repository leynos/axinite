//! Tests for Claude filesystem setup utilities.

use rstest::rstest;

use super::{build_permission_settings, copy_dir_recursive};

fn parse_allow_list(tools: &[String]) -> Vec<serde_json::Value> {
    let json_str = build_permission_settings(tools).expect("permission settings should build");
    let parsed: serde_json::Value =
        serde_json::from_str(&json_str).expect("settings JSON should parse");
    parsed["permissions"]["allow"]
        .as_array()
        .expect("allow list should be an array")
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

#[test]
fn test_copy_dir_recursive_propagates_destination_errors() {
    let src = tempfile::tempdir().expect("create src tempdir");
    let dst = tempfile::tempdir().expect("create dst tempdir");

    std::fs::create_dir_all(src.path().join("subdir")).expect("create source subdir");
    std::fs::write(src.path().join("subdir").join("nested.txt"), "nested")
        .expect("write nested source file");
    std::fs::write(dst.path().join("subdir"), "not a directory")
        .expect("block destination subdir path");

    let error = copy_dir_recursive(src.path(), dst.path())
        .expect_err("destination-side failures should be returned");
    assert_ne!(
        error.kind(),
        std::io::ErrorKind::NotFound,
        "destination errors should not be downgraded to missing source"
    );
}
