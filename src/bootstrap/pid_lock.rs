//! PID lock helpers for preventing multiple IronClaw instances.

use std::path::PathBuf;

use crate::bootstrap::ironclaw_base_dir;

/// Path to the PID lock file: `~/.ironclaw/ironclaw.pid`.
pub fn pid_lock_path() -> PathBuf {
    ironclaw_base_dir().join("ironclaw.pid")
}

/// A PID-based lock that prevents multiple IronClaw instances from running
/// simultaneously.
///
/// Uses `fs4::try_lock_exclusive()` for atomic locking (no TOCTOU race),
/// then writes the current PID into the locked file for diagnostics.
/// The OS-level lock is held for the lifetime of this struct and
/// automatically released on drop (along with the PID file cleanup).
#[derive(Debug)]
pub struct PidLock {
    path: PathBuf,
    /// Held open to maintain the OS-level exclusive lock.
    _file: std::fs::File,
}

/// Errors from PID lock acquisition.
#[derive(Debug, thiserror::Error)]
pub enum PidLockError {
    #[error("Another IronClaw instance is already running (PID {pid})")]
    AlreadyRunning { pid: u32 },
    #[error("Failed to acquire PID lock: {0}")]
    Io(#[from] std::io::Error),
}

impl PidLock {
    /// Try to acquire the PID lock.
    ///
    /// Uses an exclusive file lock (`flock`/`LockFileEx`) so that two
    /// concurrent processes cannot both acquire the lock — no TOCTOU race.
    /// If the lock file exists but the holding process is gone (stale),
    /// the lock is reclaimed automatically by the OS.
    pub fn acquire() -> Result<Self, PidLockError> {
        Self::acquire_at(pid_lock_path())
    }

    /// Acquire at a specific path (for testing).
    pub(super) fn acquire_at(path: PathBuf) -> Result<Self, PidLockError> {
        use fs4::FileExt;
        use std::fs::OpenOptions;
        use std::io::Write;

        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        let mut file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(&path)?;

        if let Err(error) = file.try_lock_exclusive() {
            if error.kind() == std::io::ErrorKind::WouldBlock {
                let pid = std::fs::read_to_string(&path)
                    .ok()
                    .and_then(|contents| contents.trim().parse::<u32>().ok())
                    .unwrap_or(0);
                return Err(PidLockError::AlreadyRunning { pid });
            }
            return Err(PidLockError::Io(error));
        }

        file.set_len(0)?;
        write!(file, "{}", std::process::id())?;

        Ok(PidLock { path, _file: file })
    }
}

impl Drop for PidLock {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}
