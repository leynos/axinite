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
