//! Tests for computing the Axinite base directory and related paths.

use std::path::PathBuf;

use crate::testing::test_utils::EnvVarsGuard;

use super::super::*;

#[test]
fn test_axinite_env_path() {
    let path = axinite_env_path();
    assert!(path.ends_with(".axinite/.env"));
}

#[test]
fn test_axinite_base_dir_default() {
    let mut env_guard = EnvVarsGuard::new(&["AXINITE_BASE_DIR"]);
    env_guard.remove("AXINITE_BASE_DIR");

    let path = compute_axinite_base_dir();
    let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("."));
    assert_eq!(path, home.join(".axinite"));
}

#[test]
fn test_axinite_base_dir_env_override() {
    let mut env_guard = EnvVarsGuard::new(&["AXINITE_BASE_DIR"]);
    env_guard.set("AXINITE_BASE_DIR", "/custom/axinite/path");

    let path = compute_axinite_base_dir();
    assert_eq!(path, PathBuf::from("/custom/axinite/path"));
}

#[test]
fn test_compute_base_dir_env_path_join() {
    let mut env_guard = EnvVarsGuard::new(&["AXINITE_BASE_DIR"]);
    env_guard.set("AXINITE_BASE_DIR", "/my/custom/dir");

    let base_path = compute_axinite_base_dir();
    let env_path = base_path.join(".env");
    assert_eq!(env_path, PathBuf::from("/my/custom/dir/.env"));
}

#[test]
fn test_axinite_base_dir_empty_env() {
    let mut env_guard = EnvVarsGuard::new(&["AXINITE_BASE_DIR"]);
    env_guard.set("AXINITE_BASE_DIR", "");

    let path = compute_axinite_base_dir();
    let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("."));
    assert_eq!(path, home.join(".axinite"));
}

#[test]
fn test_axinite_base_dir_special_chars() {
    let mut env_guard = EnvVarsGuard::new(&["AXINITE_BASE_DIR"]);
    env_guard.set("AXINITE_BASE_DIR", "/tmp/test_with-special.chars");

    let path = compute_axinite_base_dir();
    assert_eq!(path, PathBuf::from("/tmp/test_with-special.chars"));
}
