//! Tests for the bootstrap PID lock helper.

use std::path::PathBuf;
use std::process::Command;
use std::thread;
use std::time::{Duration, Instant};

use tempfile::tempdir;

use super::super::*;

#[test]
fn test_pid_lock_acquire_and_drop() {
    let dir = tempdir().expect("create temp dir for pid lock acquire/drop");
    let pid_path = dir.path().join("ironclaw.pid");

    let lock = PidLock::acquire_at(pid_path.clone()).expect("acquire initial pid lock");
    assert!(pid_path.exists());

    let contents = std::fs::read_to_string(&pid_path).expect("read pid lock file");
    assert_eq!(
        contents.trim().parse::<u32>().expect("parse current pid"),
        std::process::id()
    );

    drop(lock);
    assert!(!pid_path.exists());
}

#[test]
fn test_pid_lock_rejects_second_acquire() {
    let dir = tempdir().expect("create temp dir for double-acquire test");
    let pid_path = dir.path().join("ironclaw.pid");

    let _lock1 = PidLock::acquire_at(pid_path.clone()).expect("acquire first pid lock");

    let result = PidLock::acquire_at(pid_path.clone());
    assert!(result.is_err());
    match result.expect_err("second pid lock acquire should fail") {
        PidLockError::AlreadyRunning { pid } => assert_eq!(pid, std::process::id()),
        other => panic!("expected AlreadyRunning, got: {}", other),
    }
}

#[test]
fn test_pid_lock_reclaims_after_drop() {
    let dir = tempdir().expect("create temp dir for reclaim-after-drop test");
    let pid_path = dir.path().join("ironclaw.pid");

    let lock = PidLock::acquire_at(pid_path.clone()).expect("acquire pid lock");
    drop(lock);

    let lock2 = PidLock::acquire_at(pid_path).expect("reacquire pid lock after drop");
    drop(lock2);
}

#[test]
fn test_pid_lock_reclaims_stale_file_without_flock() {
    let dir = tempdir().expect("create temp dir for stale-file pid lock test");
    let pid_path = dir.path().join("ironclaw.pid");

    std::fs::write(&pid_path, "4294967294").expect("write stale pid file");

    let lock = PidLock::acquire_at(pid_path.clone()).expect("reclaim stale pid lock");
    let contents = std::fs::read_to_string(&pid_path).expect("read reclaimed pid lock");
    assert_eq!(
        contents.trim().parse::<u32>().expect("parse reclaimed pid"),
        std::process::id()
    );
    drop(lock);
}

#[test]
fn test_pid_lock_handles_corrupt_pid_file() {
    let dir = tempdir().expect("create temp dir for corrupt pid test");
    let pid_path = dir.path().join("ironclaw.pid");

    std::fs::write(&pid_path, "not-a-number").expect("write corrupt pid file");

    let lock = PidLock::acquire_at(pid_path).expect("reclaim corrupt pid file");
    drop(lock);
}

#[test]
fn test_pid_lock_creates_parent_dirs() {
    let dir = tempdir().expect("create temp dir for nested pid lock test");
    let pid_path = dir.path().join("nested").join("deep").join("ironclaw.pid");

    let lock = PidLock::acquire_at(pid_path.clone()).expect("acquire nested pid lock");
    assert!(pid_path.exists());
    drop(lock);
}

#[test]
fn test_pid_lock_child_helper_holds_lock() {
    if std::env::var("IRONCLAW_PID_LOCK_CHILD").ok().as_deref() != Some("1") {
        return;
    }

    let pid_path = PathBuf::from(
        std::env::var("IRONCLAW_PID_LOCK_PATH").expect("IRONCLAW_PID_LOCK_PATH missing"),
    );
    let hold_ms = std::env::var("IRONCLAW_PID_LOCK_HOLD_MS")
        .ok()
        .and_then(|value| value.parse::<u64>().ok())
        .unwrap_or(3000);

    let _lock = PidLock::acquire_at(pid_path).expect("child failed to acquire pid lock");
    thread::sleep(Duration::from_millis(hold_ms));
}

#[test]
fn test_pid_lock_rejects_lock_held_by_other_process() {
    let dir = tempdir().expect("create temp dir for external pid lock test");
    let pid_path = dir.path().join("ironclaw.pid");
    let current_exe = std::env::current_exe().expect("locate current test binary");
    let mut child = Command::new(current_exe)
        .args([
            "--exact",
            "bootstrap::tests::pid_lock::test_pid_lock_child_helper_holds_lock",
            "--nocapture",
            "--test-threads=1",
        ])
        .env("IRONCLAW_PID_LOCK_CHILD", "1")
        .env("IRONCLAW_PID_LOCK_PATH", pid_path.display().to_string())
        .env("IRONCLAW_PID_LOCK_HOLD_MS", "3000")
        .spawn()
        .expect("spawn child test process");

    let started = Instant::now();
    while started.elapsed() < Duration::from_secs(2) {
        if pid_path.exists() {
            break;
        }
        if let Some(status) = child.try_wait().expect("poll child pid lock test") {
            panic!("child exited before acquiring lock: {}", status);
        }
        thread::sleep(Duration::from_millis(20));
    }
    assert!(
        pid_path.exists(),
        "child did not create lock file in time: {}",
        pid_path.display()
    );

    let result = PidLock::acquire_at(pid_path.clone());
    match result.expect_err("pid lock should be held by child process") {
        PidLockError::AlreadyRunning { .. } => {}
        other => panic!("expected AlreadyRunning, got: {}", other),
    }

    let status = child.wait().expect("wait for child pid lock process");
    assert!(status.success(), "child process failed: {}", status);

    let lock = PidLock::acquire_at(pid_path).expect("reacquire pid lock after child exit");
    drop(lock);
}
