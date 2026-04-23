//! Cleanup helper tests.

use crate::support::cleanup::CleanupGuard;

#[test]
fn cleanup_guard_removes_file() {
    let path = "/tmp/ironclaw_cleanup_guard_test.txt";
    std::fs::write(path, "test").unwrap();
    {
        let _guard = CleanupGuard::new().file(path);
        assert!(std::path::Path::new(path).exists());
    }
    assert!(!std::path::Path::new(path).exists());
}

#[test]
fn cleanup_guard_removes_dir() {
    let dir = "/tmp/ironclaw_cleanup_guard_test_dir";
    std::fs::create_dir_all(dir).unwrap();
    std::fs::write(format!("{dir}/file.txt"), "test").unwrap();
    {
        let _guard = CleanupGuard::new().dir(dir);
        assert!(std::path::Path::new(dir).exists());
    }
    assert!(!std::path::Path::new(dir).exists());
}

#[test]
fn cleanup_guard_file_does_not_remove_dir() {
    let dir = "/tmp/ironclaw_cleanup_guard_file_not_dir";
    std::fs::create_dir_all(dir).unwrap();
    {
        let _guard = CleanupGuard::new().file(dir);
    }
    assert!(
        std::path::Path::new(dir).exists(),
        "dir should still exist when registered as file"
    );
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn setup_test_dir_creates_missing_directory() {
    let dir = tempfile::tempdir().expect("failed to create tempdir");
    let target = dir.path().join("nested");
    let target_str = target.to_string_lossy().into_owned();

    crate::support::cleanup::setup_test_dir(&target_str).expect("setup_test_dir failed");

    assert!(
        target.is_dir(),
        "setup_test_dir should create the directory"
    );
}

#[test]
fn setup_test_dir_with_suffix_creates_unique_directory() {
    let dir = tempfile::tempdir().expect("failed to create tempdir");

    let created = crate::support::cleanup::setup_test_dir_with_suffix(dir.path(), "cleanup-tests")
        .expect("setup_test_dir_with_suffix failed");

    assert!(
        std::path::Path::new(&created).is_dir(),
        "setup_test_dir_with_suffix should create the directory"
    );
    assert!(
        created.contains("cleanup-tests"),
        "created path should include the requested suffix"
    );
}
