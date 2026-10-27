//! Tests for computing the Axinite base directory and related paths.

use std::collections::HashMap;
use std::path::PathBuf;

use crate::config::EnvContext;

use super::super::*;

#[test]
fn test_axinite_base_dir_default() {
    let context = EnvContext::for_testing(
        HashMap::from([(String::from("HOME"), String::from("/home/test-user"))]),
        HashMap::new(),
    );
    let path = compute_axinite_base_dir_from(&context);
    assert_eq!(path, PathBuf::from("/home/test-user/.axinite"));
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
    let ctx = EnvContext::for_testing(
        HashMap::from([
            (String::from("AXINITE_BASE_DIR"), String::new()),
            (String::from("HOME"), String::from("/home/test-user")),
        ]),
        HashMap::new(),
    );
    let path = compute_axinite_base_dir_from(&ctx);
    assert_eq!(path, PathBuf::from("/home/test-user/.axinite"));
}

#[test]
fn test_axinite_base_dir_special_chars() {
    let ctx = EnvContext::default().with_env("AXINITE_BASE_DIR", "/tmp/test_with-special.chars");
    let path = compute_axinite_base_dir_from(&ctx);
    assert_eq!(path, PathBuf::from("/tmp/test_with-special.chars"));
}
