//! Cleanup helper tests.

use crate::support::cleanup::CleanupGuard;

#[test]
fn cleanup_guard_removes_file() {
    let file = tempfile::NamedTempFile::new().expect("should create temp file");
    let path = file.path().to_string_lossy().into_owned();
    std::fs::write(&path, "test").expect("should write test file");
    {
        let _guard = CleanupGuard::new().file(path.clone());
        assert!(std::path::Path::new(&path).exists());
    }
    assert!(!std::path::Path::new(&path).exists());
}

#[test]
fn cleanup_guard_removes_dir() {
    let dir = tempfile::tempdir().expect("should create temp dir");
    let path = dir.path().to_string_lossy().into_owned();
    std::fs::write(dir.path().join("file.txt"), "test").expect("should write file in test dir");
    {
        let _guard = CleanupGuard::new().dir(path.clone());
        assert!(std::path::Path::new(&path).exists());
    }
    assert!(!std::path::Path::new(&path).exists());
}

#[test]
fn cleanup_guard_file_does_not_remove_dir() {
    let dir = tempfile::tempdir().expect("should create temp dir");
    let path = dir.path().to_string_lossy().into_owned();
    {
        let _guard = CleanupGuard::new().file(path.clone());
    }
    assert!(
        std::path::Path::new(&path).exists(),
        "dir should still exist when registered as file"
    );
}

#[test]
fn setup_test_dir_creates_missing_directory() {
    let dir = tempfile::tempdir().expect("should create temp dir");
    let target = dir.path().join("nested");
    let target_str = target.to_string_lossy().into_owned();

    crate::support::cleanup::setup_test_dir(&target_str).expect("setup_test_dir should succeed");

    assert!(
        target.is_dir(),
        "setup_test_dir should create the directory"
    );
}

#[test]
fn setup_test_dir_with_suffix_creates_unique_directory() {
    let dir = tempfile::tempdir().expect("should create temp dir");

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
