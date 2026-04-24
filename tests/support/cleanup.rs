//! RAII cleanup guard and directory-setup helpers for test directories and
//! files.

use std::path::Path;

#[path = "cleanup_guard.rs"]
mod cleanup_guard;

pub use cleanup_guard::CleanupGuard;

/// Remove and recreate a test directory, ensuring a clean slate.
pub fn setup_test_dir(path: &str) -> std::io::Result<()> {
    match std::fs::remove_dir_all(path) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(error),
    }

    std::fs::create_dir_all(path)
}

/// Remove and recreate a suffixed test directory, returning the full path.
///
/// Useful when tests need isolated directories to avoid collisions.
pub fn setup_test_dir_with_suffix(base: &Path, suffix: &str) -> std::io::Result<String> {
    let base = base
        .to_str()
        .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::InvalidData, "non-UTF-8 path"))?;
    let dir = format!("{base}_{suffix}");
    setup_test_dir(&dir)?;
    Ok(dir)
}
