//! RAII cleanup guard and directory-setup helpers for test directories and
//! files.

use std::path::Path;

/// The kind of path registered for cleanup.
enum PathKind {
    File,
    Dir,
}

/// Removes listed paths when dropped, ensuring cleanup even on panic.
pub struct CleanupGuard {
    paths: Vec<(String, PathKind)>,
}

impl CleanupGuard {
    pub fn new() -> Self {
        Self { paths: Vec::new() }
    }

    /// Register a file path for cleanup on drop.
    pub fn file(mut self, path: impl Into<String>) -> Self {
        self.paths.push((path.into(), PathKind::File));
        self
    }

    /// Register a directory path for cleanup on drop.
    pub fn dir(mut self, path: impl Into<String>) -> Self {
        self.paths.push((path.into(), PathKind::Dir));
        self
    }
}

impl Drop for CleanupGuard {
    fn drop(&mut self) {
        for (path, kind) in &self.paths {
            match kind {
                PathKind::File => {
                    let _ = std::fs::remove_file(path);
                }
                PathKind::Dir => {
                    let _ = std::fs::remove_dir_all(path);
                }
            }
        }
    }
}

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
    let dir = format!("{}_{suffix}", base.to_string_lossy());
    setup_test_dir(&dir)?;
    Ok(dir)
}
