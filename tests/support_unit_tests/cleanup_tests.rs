//! Cleanup helper tests.

use crate::support::cleanup::CleanupGuard;
use rstest::{fixture, rstest};

#[fixture]
fn tmp_dir_fixture() -> tempfile::TempDir {
    tempfile::tempdir().expect("should create temp dir")
}

/// Asserts that `CleanupGuard` removes the resource at `path` when it is dropped.
///
/// `register` should attach `path` to the guard using the appropriate
/// registration method (`.file()` or `.dir()`) and return the updated guard.
fn assert_guard_removes_at_path<F>(path: &str, register: F)
where
    F: FnOnce(CleanupGuard) -> CleanupGuard,
{
    {
        let _guard = register(CleanupGuard::new());
        assert!(
            std::path::Path::new(path).exists(),
            "path should exist while guard is held"
        );
    }
    assert!(
        !std::path::Path::new(path).exists(),
        "path should not exist after guard drops"
    );
}

#[test]
fn cleanup_guard_removes_file() {
    let dir = tempfile::tempdir().expect("should create temp dir");
    let path = dir.path().join("guarded-file.txt");
    let path = path.to_string_lossy().into_owned();
    std::fs::write(&path, "test").expect("should write test file");
    assert_guard_removes_at_path(&path, |g| g.file(path.clone()));
}

#[rstest]
fn cleanup_guard_removes_dir(tmp_dir_fixture: tempfile::TempDir) {
    let dir = tmp_dir_fixture;
    let path = dir.path().to_string_lossy().into_owned();
    std::fs::write(dir.path().join("file.txt"), "test").expect("should write file in test dir");
    assert_guard_removes_at_path(&path, |g| g.dir(path.clone()));
}

#[rstest]
fn cleanup_guard_file_does_not_remove_dir(tmp_dir_fixture: tempfile::TempDir) {
    let dir = tmp_dir_fixture;
    let path = dir.path().to_string_lossy().into_owned();
    {
        let _guard = CleanupGuard::new().file(path.clone());
    }
    assert!(
        std::path::Path::new(&path).exists(),
        "dir should still exist when registered as file"
    );
}

#[rstest]
fn setup_test_dir_creates_missing_directory(tmp_dir_fixture: tempfile::TempDir) {
    let dir = tmp_dir_fixture;
    let target = dir.path().join("nested");
    let target_str = target.to_string_lossy().into_owned();

    crate::support::cleanup::setup_test_dir(&target_str).expect("setup_test_dir should succeed");

    assert!(
        target.is_dir(),
        "setup_test_dir should create the directory"
    );
}

#[rstest]
fn setup_test_dir_with_suffix_creates_unique_directory(tmp_dir_fixture: tempfile::TempDir) {
    let dir = tmp_dir_fixture;

    let created = crate::support::cleanup::setup_test_dir_with_suffix(dir.path(), "cleanup-tests")
        .expect("setup_test_dir_with_suffix should succeed");
    let _guard = CleanupGuard::new().dir(created.clone());
    let created2 = crate::support::cleanup::setup_test_dir_with_suffix(dir.path(), "cleanup-tests")
        .expect("setup_test_dir_with_suffix should succeed");
    let _guard2 = CleanupGuard::new().dir(created2.clone());

    assert!(
        std::path::Path::new(&created).is_dir(),
        "setup_test_dir_with_suffix should create the directory"
    );
    assert!(
        created.contains("cleanup-tests"),
        "created path should include the requested suffix"
    );
    assert!(
        std::path::Path::new(&created2).is_dir(),
        "setup_test_dir_with_suffix should create the second directory"
    );
    assert!(
        created2.contains("cleanup-tests"),
        "second created path should include the requested suffix"
    );
    assert_ne!(
        created, created2,
        "setup_test_dir_with_suffix should create unique directories"
    );
}
