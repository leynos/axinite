//! Process coordination for the nextest boundary fixture.
//!
//! Its integration tests share a marker file so the workflow contract can
//! detect overlap between separate nextest test processes.

use std::fs::{self, OpenOptions};
use std::io;
use std::path::PathBuf;
use std::time::Duration;

const OVERLAP_MARKER_ENV: &str = "AXINITE_NEXTEST_OVERLAP_MARKER";
const OVERLAP_WINDOW: Duration = Duration::from_millis(350);

struct MarkerGuard(PathBuf);

impl Drop for MarkerGuard {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.0);
    }
}

/// Hold an exclusive marker long enough to expose concurrent test starts.
///
/// The delay is an observation window. The workflow contract asserts on the
/// `create_new` collision, not on the nextest command's elapsed time.
pub fn detect_test_process_overlap() -> io::Result<()> {
    let Some(marker_path) = std::env::var_os(OVERLAP_MARKER_ENV) else {
        return Ok(());
    };
    let marker_path = PathBuf::from(marker_path);
    let marker_file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&marker_path)
        .map_err(|error| {
            if error.kind() == io::ErrorKind::AlreadyExists {
                io::Error::new(
                    io::ErrorKind::WouldBlock,
                    format!(
                        "compile-contract test processes overlapped at {}",
                        marker_path.display()
                    ),
                )
            } else {
                error
            }
        })?;
    drop(marker_file);
    let _guard = MarkerGuard(marker_path);
    std::thread::sleep(OVERLAP_WINDOW);
    Ok(())
}
