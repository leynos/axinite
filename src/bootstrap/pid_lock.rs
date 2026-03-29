//! PID lock helpers for preventing multiple IronClaw instances.

use std::ffi::OsString;
use std::path::PathBuf;

use cap_std::ambient_authority;
use cap_std::fs::{Dir, File, OpenOptions};

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
    parent_dir: PathBuf,
    file_name: OsString,
    /// Held open to maintain the OS-level exclusive lock.
    _file: File,
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
        use std::io::Write;
        let parent = path.parent().ok_or_else(|| {
            std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                format!("PID lock path has no parent: {}", path.display()),
            )
        })?;
        let file_name = path.file_name().ok_or_else(|| {
            std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                format!("PID lock path has no file name: {}", path.display()),
            )
        })?;

        std::fs::create_dir_all(parent)?;
        let parent_dir = Dir::open_ambient_dir(parent, ambient_authority())?;
        let mut options = OpenOptions::new();
        options.read(true).write(true).create(true).truncate(false);
        let mut file = parent_dir.open_with(file_name, &options)?;

        if let Err(error) = try_lock_exclusive(&file) {
            if error.kind() == std::io::ErrorKind::WouldBlock {
                let pid = parent_dir
                    .read_to_string(file_name)
                    .ok()
                    .and_then(|contents| contents.trim().parse::<u32>().ok())
                    .unwrap_or(0);
                return Err(PidLockError::AlreadyRunning { pid });
            }
            return Err(PidLockError::Io(error));
        }

        file.set_len(0)?;
        write!(file, "{}", std::process::id())?;

        Ok(PidLock {
            parent_dir: parent.to_path_buf(),
            file_name: file_name.to_os_string(),
            _file: file,
        })
    }
}

impl Drop for PidLock {
    fn drop(&mut self) {
        if let Ok(parent_dir) = Dir::open_ambient_dir(&self.parent_dir, ambient_authority()) {
            let _ = parent_dir.remove_file(&self.file_name);
        }
    }
}

#[cfg(unix)]
fn try_lock_exclusive(file: &File) -> std::io::Result<()> {
    use std::os::fd::AsRawFd;

    // SAFETY: `file` is a valid, open file descriptor for the duration of the
    // call, and `libc::flock` does not take ownership of the descriptor.
    let result = unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) };
    if result == 0 {
        Ok(())
    } else {
        Err(std::io::Error::last_os_error())
    }
}

#[cfg(windows)]
fn try_lock_exclusive(file: &File) -> std::io::Result<()> {
    use std::os::windows::io::AsRawHandle;

    #[repr(C)]
    struct Overlapped {
        internal: usize,
        internal_high: usize,
        offset: u32,
        offset_high: u32,
        h_event: *mut core::ffi::c_void,
    }

    unsafe extern "system" {
        fn LockFileEx(
            h_file: *mut core::ffi::c_void,
            dw_flags: u32,
            dw_reserved: u32,
            n_number_of_bytes_to_lock_low: u32,
            n_number_of_bytes_to_lock_high: u32,
            lp_overlapped: *mut Overlapped,
        ) -> i32;
    }

    const LOCKFILE_EXCLUSIVE_LOCK: u32 = 0x00000002;
    const LOCKFILE_FAIL_IMMEDIATELY: u32 = 0x00000001;
    let mut overlapped = Overlapped {
        internal: 0,
        internal_high: 0,
        offset: 0,
        offset_high: 0,
        h_event: core::ptr::null_mut(),
    };

    // SAFETY: `file` is an open handle, `overlapped` lives for the duration of
    // the call, and the API does not take ownership of the handle.
    let result = unsafe {
        LockFileEx(
            file.as_raw_handle().cast(),
            LOCKFILE_EXCLUSIVE_LOCK | LOCKFILE_FAIL_IMMEDIATELY,
            0,
            u32::MAX,
            u32::MAX,
            &mut overlapped,
        )
    };
    if result != 0 {
        Ok(())
    } else {
        Err(std::io::Error::last_os_error())
    }
}
