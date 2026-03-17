//! RAII cleanup guard and directory-setup helpers for test directories and
//! files.

/// The kind of path registered for cleanup.
enum PathKind {
    File,
    Dir,
}

/// Removes listed paths when dropped, ensuring cleanup even on panic.
#[allow(dead_code)]
pub struct CleanupGuard {
    paths: Vec<(String, PathKind)>,
}

#[allow(dead_code)]
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
#[allow(dead_code)]
pub fn setup_test_dir(path: &str) {
    // Ignore error: directory may not exist, removal failures are non-fatal in tests
    let _ = std::fs::remove_dir_all(path);
    std::fs::create_dir_all(path).expect("failed to create test directory");
}

/// Remove and recreate a suffixed test directory, returning the full path.
///
/// Useful when tests need isolated directories to avoid collisions.
#[allow(dead_code)]
pub fn setup_test_dir_with_suffix(base: &str, suffix: &str) -> String {
    let dir = format!("{base}_{suffix}");
    setup_test_dir(&dir);
    dir
}
