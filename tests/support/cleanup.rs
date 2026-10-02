//! RAII cleanup guard and directory-setup helpers for test directories and
//! files.

use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};

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
    let unique = next_unique_dir_sequence().ok_or_else(|| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "test directory uniqueness counter overflowed",
        )
    })?;
    let dir = format!("{base}_{suffix}_{}_{}", std::process::id(), unique);
    setup_test_dir(&dir)?;
    Ok(dir)
}

static NEXT_UNIQUE_DIR: AtomicU64 = AtomicU64::new(0);

fn next_unique_dir_sequence() -> Option<u64> {
    let mut current = NEXT_UNIQUE_DIR.load(Ordering::Relaxed);
    loop {
        let next = current.checked_add(1)?;
        match NEXT_UNIQUE_DIR.compare_exchange_weak(
            current,
            next,
            Ordering::Relaxed,
            Ordering::Relaxed,
        ) {
            Ok(_) => return Some(current),
            Err(observed) => current = observed,
        }
    }
}

// `NEXT_UNIQUE_DIR` overflow in `setup_test_dir_with_suffix` would require
// exhausting `u64::MAX` successful calls in one process. The path is guarded
// for correctness, but it cannot be practically triggered in tests without
// exposing a test-only mutation hook for production cleanup state.
