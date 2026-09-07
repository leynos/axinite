//! Tests for WASM artifact discovery and installation helpers.

use std::path::Path;

use crate::config::EnvContext;
use crate::registry::artifacts::{
    WASM_TRIPLES, find_any_wasm_artifact_from, find_wasm_artifact_from, install_wasm_files,
    resolve_target_dir_from,
};
use tempfile::TempDir;

use super::SHARED_WASM_TARGET_DIR;

#[test]
fn test_resolve_target_dir_default() {
    let dir = Path::new("/some/crate");
    let result = resolve_target_dir_from(dir, &EnvContext::default());
    assert_eq!(result, dir.join("target"));
}

#[test]
fn test_resolve_target_dir_relative_env_path() {
    let ctx = EnvContext::default().with_env("CARGO_TARGET_DIR", "target-relative");
    let dir = Path::new("/some/crate");
    let result = resolve_target_dir_from(dir, &ctx);
    assert_eq!(
        result,
        std::env::current_dir()
            .expect("resolve current directory")
            .join("target-relative")
    );
}

#[test]
fn test_find_wasm_artifact_falls_back_to_repo_shared_target_dir() {
    let repo = TempDir::new().expect("create temp dir");
    let crate_dir = repo.path().join("channels-src/demo");
    let shared_target = repo.path().join(SHARED_WASM_TARGET_DIR);
    let wasm_dir = shared_target.join("wasm32-wasip2/release");

    ambient_fs::create_dir_all(&crate_dir).expect("create crate dir");
    ambient_fs::create_dir_all(&wasm_dir).expect("create shared wasm dir");
    ambient_fs::File::create(wasm_dir.join("demo_channel.wasm"))
        .expect("create shared wasm artifact");

    let result = find_wasm_artifact_from(
        &crate_dir,
        "demo-channel",
        "release",
        &EnvContext::default(),
    );
    assert_eq!(
        result.expect("find demo-channel artifact"),
        wasm_dir.join("demo_channel.wasm")
    );
}

#[test]
fn test_find_wasm_artifact_prefers_repo_shared_target_dir_over_crate_target() {
    let repo = TempDir::new().expect("create temp dir");
    let crate_dir = repo.path().join("channels-src/demo");
    let shared_wasm_dir = repo
        .path()
        .join(SHARED_WASM_TARGET_DIR)
        .join("wasm32-wasip2/release");
    let crate_wasm_dir = crate_dir.join("target/wasm32-wasip2/release");

    ambient_fs::create_dir_all(&crate_wasm_dir).expect("create crate wasm dir");
    ambient_fs::create_dir_all(&shared_wasm_dir).expect("create shared wasm dir");
    ambient_fs::write(crate_wasm_dir.join("demo_channel.wasm"), b"crate-local")
        .expect("write crate-local wasm");
    ambient_fs::write(shared_wasm_dir.join("demo_channel.wasm"), b"shared")
        .expect("write shared wasm");

    let result = find_wasm_artifact_from(
        &crate_dir,
        "demo-channel",
        "release",
        &EnvContext::default(),
    )
    .expect("find demo-channel artifact");
    assert_eq!(result, shared_wasm_dir.join("demo_channel.wasm"));
    assert_eq!(
        ambient_fs::read(&result).expect("read resolved wasm"),
        b"shared"
    );
}

#[test]
fn test_find_wasm_artifact_not_found() {
    let dir = TempDir::new().expect("create temp dir");
    assert!(
        find_wasm_artifact_from(dir.path(), "nonexistent", "release", &EnvContext::default(),)
            .is_none()
    );
}

#[test]
fn test_find_wasm_artifact_found() {
    let dir = TempDir::new().expect("create temp dir");
    let ctx = EnvContext::default();
    let target_base = resolve_target_dir_from(dir.path(), &ctx);
    let wasm_dir = target_base.join("wasm32-wasip2/release");
    ambient_fs::create_dir_all(&wasm_dir).expect("create wasm32-wasip2 dir");
    ambient_fs::File::create(wasm_dir.join("my_tool.wasm")).expect("create wasm artifact");

    let result = find_wasm_artifact_from(dir.path(), "my_tool", "release", &ctx)
        .expect("find my_tool artifact in wasm32-wasip2 target dir");
    assert!(result.ends_with("my_tool.wasm"));
}

#[test]
fn test_find_wasm_artifact_hyphen_to_underscore() {
    let dir = TempDir::new().expect("create temp dir");
    let ctx = EnvContext::default();
    let target_base = resolve_target_dir_from(dir.path(), &ctx);
    let wasm_dir = target_base.join("wasm32-wasip1/release");
    ambient_fs::create_dir_all(&wasm_dir).expect("create wasm32-wasip1 dir");
    ambient_fs::File::create(wasm_dir.join("my_tool.wasm")).expect("create wasm artifact");

    let result = find_wasm_artifact_from(dir.path(), "my-tool", "release", &ctx)
        .expect("find my-tool artifact after hyphen-to-underscore normalisation");
    assert!(result.ends_with("my_tool.wasm"));
}

#[test]
fn test_find_wasm_artifact_prefers_wasip2_over_wasip1() {
    let dir = TempDir::new().expect("temp dir");
    let ctx = EnvContext::default();
    let target_base = resolve_target_dir_from(dir.path(), &ctx);
    let wasip1_dir = target_base.join("wasm32-wasip1/release");
    let wasip2_dir = target_base.join("wasm32-wasip2/release");
    ambient_fs::create_dir_all(&wasip1_dir).expect("create wasip1 dir");
    ambient_fs::create_dir_all(&wasip2_dir).expect("create wasip2 dir");
    ambient_fs::File::create(wasip1_dir.join("my_tool.wasm")).expect("create wasip1 wasm");
    ambient_fs::File::create(wasip2_dir.join("my_tool.wasm")).expect("create wasip2 wasm");

    let result = find_wasm_artifact_from(dir.path(), "my_tool", "release", &ctx)
        .expect("should find wasm artifact");
    assert!(
        result.ends_with("wasm32-wasip2/release/my_tool.wasm"),
        "expected wasm32-wasip2 artifact, got {}",
        result.display()
    );
}

#[test]
fn test_find_any_wasm_artifact_found() {
    let dir = TempDir::new().expect("create temp dir");
    let ctx = EnvContext::default();
    let target_base = resolve_target_dir_from(dir.path(), &ctx);
    let wasm_dir = target_base.join("wasm32-wasip2/release");
    ambient_fs::create_dir_all(&wasm_dir).expect("create wasm dir");
    ambient_fs::File::create(wasm_dir.join("something.wasm")).expect("create wasm artifact");

    let result = find_any_wasm_artifact_from(dir.path(), "release", &ctx)
        .expect("find any wasm artifact in release target dir");
    assert!(result.ends_with("something.wasm"));
}

#[test]
fn test_find_any_wasm_artifact_not_found() {
    let dir = TempDir::new().expect("create temp dir");
    assert!(find_any_wasm_artifact_from(dir.path(), "release", &EnvContext::default()).is_none());
}

#[tokio::test]
async fn test_install_wasm_files_copies() {
    let src_dir = TempDir::new().expect("create source temp dir");
    let target_dir = TempDir::new().expect("create target temp dir");

    let wasm_src = src_dir.path().join("test.wasm");
    tokio::fs::write(&wasm_src, b"\0asm\x01\x00\x00\x00")
        .await
        .expect("write source wasm");

    let caps_src = src_dir.path().join("mytool.capabilities.json");
    tokio::fs::write(&caps_src, b"{}")
        .await
        .expect("write capabilities file");

    let wasm_dst = install_wasm_files(
        &wasm_src,
        src_dir.path(),
        "mytool",
        target_dir.path(),
        false,
    )
    .await
    .expect("install wasm files into empty target directory");
    assert!(wasm_dst.exists());
    assert!(target_dir.path().join("mytool.capabilities.json").exists());
}

#[tokio::test]
async fn test_install_wasm_files_refuses_overwrite() {
    let src_dir = TempDir::new().expect("create source temp dir");
    let target_dir = TempDir::new().expect("create target temp dir");

    let wasm_src = src_dir.path().join("test.wasm");
    tokio::fs::write(&wasm_src, b"\0asm")
        .await
        .expect("write source wasm");

    let existing = target_dir.path().join("mytool.wasm");
    tokio::fs::write(&existing, b"existing")
        .await
        .expect("write existing target wasm");

    let result = install_wasm_files(
        &wasm_src,
        src_dir.path(),
        "mytool",
        target_dir.path(),
        false,
    )
    .await;

    assert!(result.is_err());
}

#[test]
fn test_wasm_triples_order() {
    assert_eq!(WASM_TRIPLES[0], "wasm32-wasip2");
    assert_eq!(WASM_TRIPLES[1], "wasm32-wasip1");
    assert_eq!(WASM_TRIPLES[2], "wasm32-wasi");
    assert_eq!(WASM_TRIPLES[3], "wasm32-unknown-unknown");
}
