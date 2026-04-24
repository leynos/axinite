//! RAII cleanup guard for test directories and files.

/// The kind of path registered for cleanup.
enum PathKind {
    File,
    Dir,
}

impl PathKind {
    fn label(&self) -> &'static str {
        match self {
            Self::File => "file",
            Self::Dir => "directory",
        }
    }
}

/// Removes listed paths when dropped, ensuring cleanup even on panic.
#[derive(Default)]
pub struct CleanupGuard {
    paths: Vec<(String, PathKind)>,
}

impl CleanupGuard {
    /// Create a default guard that removes registered paths when dropped.
    pub fn new() -> Self {
        Self::default()
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
            let result = match kind {
                PathKind::File => std::fs::remove_file(path),
                PathKind::Dir => std::fs::remove_dir_all(path),
            };

            if let Err(error) = result
                && error.kind() != std::io::ErrorKind::NotFound
            {
                eprintln!(
                    "failed to clean up test {} '{}': {}",
                    kind.label(),
                    path,
                    error
                );
            }
        }
    }
}
