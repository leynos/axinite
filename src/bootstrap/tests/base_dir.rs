//! Tests for computing the Axinite base directory and related paths.

use std::path::PathBuf;

use crate::config::EnvContext;

use super::super::*;

#[test]
fn test_axinite_env_path() {
    let path = axinite_env_path();
    assert!(path.ends_with(".axinite/.env"));
}

#[test]
fn test_axinite_base_dir_default() {
    let path = compute_axinite_base_dir_from(&EnvContext::default());
    let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("."));
    assert_eq!(path, home.join(".axinite"));
}

#[test]
fn test_axinite_base_dir_env_override() {
    let ctx = EnvContext::default().with_env("AXINITE_BASE_DIR", "/custom/axinite/path");
    let path = compute_axinite_base_dir_from(&ctx);
    assert_eq!(path, PathBuf::from("/custom/axinite/path"));
}

#[test]
fn test_compute_base_dir_env_path_join() {
    let ctx = EnvContext::default().with_env("AXINITE_BASE_DIR", "/my/custom/dir");
    let base_path = compute_axinite_base_dir_from(&ctx);
    let env_path = base_path.join(".env");
    assert_eq!(env_path, PathBuf::from("/my/custom/dir/.env"));
}

#[test]
fn test_axinite_base_dir_empty_env() {
    let ctx = EnvContext::default().with_env("AXINITE_BASE_DIR", "");
    let path = compute_axinite_base_dir_from(&ctx);
    let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("."));
    assert_eq!(path, home.join(".axinite"));
}

#[test]
fn test_axinite_base_dir_special_chars() {
    let ctx = EnvContext::default().with_env("AXINITE_BASE_DIR", "/tmp/test_with-special.chars");
    let path = compute_axinite_base_dir_from(&ctx);
    assert_eq!(path, PathBuf::from("/tmp/test_with-special.chars"));
}
