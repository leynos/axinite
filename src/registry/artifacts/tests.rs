use std::path::Path;

use crate::config::helpers::ENV_MUTEX;
use crate::registry::artifacts::{
    WASM_TRIPLES, find_any_wasm_artifact, find_wasm_artifact, install_wasm_files,
    resolve_target_dir,
};
use rstest::{fixture, rstest};
use tempfile::TempDir;

use super::SHARED_WASM_TARGET_DIR;

struct ClearedTargetDirGuard {
    _guard: std::sync::MutexGuard<'static, ()>,
    original: Option<String>,
}

impl Drop for ClearedTargetDirGuard {
    fn drop(&mut self) {
        unsafe {
            if let Some(value) = &self.original {
                std::env::set_var("CARGO_TARGET_DIR", value);
            } else {
                std::env::remove_var("CARGO_TARGET_DIR");
            }
        }
    }
}

#[fixture]
fn cleared_target_dir() -> ClearedTargetDirGuard {
    let guard = ENV_MUTEX.lock().expect("env mutex poisoned");
    let original = std::env::var("CARGO_TARGET_DIR").ok();
    unsafe {
        std::env::remove_var("CARGO_TARGET_DIR");
    }

    ClearedTargetDirGuard {
        _guard: guard,
        original,
    }
}

#[rstest]
fn test_resolve_target_dir_default(_cleared_target_dir: ClearedTargetDirGuard) {
    let dir = Path::new("/some/crate");
    let result = resolve_target_dir(dir);
    assert_eq!(result, dir.join("target"));
}

#[rstest]
fn test_find_wasm_artifact_falls_back_to_repo_shared_target_dir(
    _cleared_target_dir: ClearedTargetDirGuard,
) {
    let repo = TempDir::new().expect("create temp dir");
    let crate_dir = repo.path().join("channels-src/demo");
    let shared_target = repo.path().join(SHARED_WASM_TARGET_DIR);
    let wasm_dir = shared_target.join("wasm32-wasip2/release");

    std::fs::create_dir_all(&crate_dir).expect("create crate dir");
    std::fs::create_dir_all(&wasm_dir).expect("create shared wasm dir");
    std::fs::File::create(wasm_dir.join("demo_channel.wasm")).expect("create shared wasm artifact");

    let result = find_wasm_artifact(&crate_dir, "demo-channel", "release");
    assert_eq!(
        result.expect("find demo-channel artifact"),
        wasm_dir.join("demo_channel.wasm")
    );
}

#[rstest]
fn test_find_wasm_artifact_prefers_repo_shared_target_dir_over_crate_target(
    _cleared_target_dir: ClearedTargetDirGuard,
) {
    let repo = TempDir::new().expect("create temp dir");
    let crate_dir = repo.path().join("channels-src/demo");
    let shared_wasm_dir = repo
        .path()
        .join(SHARED_WASM_TARGET_DIR)
        .join("wasm32-wasip2/release");
    let crate_wasm_dir = crate_dir.join("target/wasm32-wasip2/release");

    std::fs::create_dir_all(&crate_wasm_dir).expect("create crate wasm dir");
    std::fs::create_dir_all(&shared_wasm_dir).expect("create shared wasm dir");
    std::fs::write(crate_wasm_dir.join("demo_channel.wasm"), b"crate-local")
        .expect("write crate-local wasm");
    std::fs::write(shared_wasm_dir.join("demo_channel.wasm"), b"shared")
        .expect("write shared wasm");

    let result = find_wasm_artifact(&crate_dir, "demo-channel", "release")
        .expect("find demo-channel artifact");
    assert_eq!(result, shared_wasm_dir.join("demo_channel.wasm"));
    assert_eq!(
        std::fs::read(&result).expect("read resolved wasm"),
        b"shared"
    );
}

#[test]
fn test_find_wasm_artifact_not_found() {
    let dir = TempDir::new().expect("create temp dir");
    assert!(find_wasm_artifact(dir.path(), "nonexistent", "release").is_none());
}

#[rstest]
fn test_find_wasm_artifact_found(_cleared_target_dir: ClearedTargetDirGuard) {
    let dir = TempDir::new().expect("create temp dir");
    let target_base = resolve_target_dir(dir.path());
    let wasm_dir = target_base.join("wasm32-wasip2/release");
    std::fs::create_dir_all(&wasm_dir).expect("create wasm32-wasip2 dir");
    std::fs::File::create(wasm_dir.join("my_tool.wasm")).expect("create wasm artifact");

    let result = find_wasm_artifact(dir.path(), "my_tool", "release");
    assert!(result.is_some());
    assert!(
        result
            .expect("find my_tool artifact")
            .ends_with("my_tool.wasm")
    );
}

#[rstest]
fn test_find_wasm_artifact_hyphen_to_underscore(_cleared_target_dir: ClearedTargetDirGuard) {
    let dir = TempDir::new().expect("create temp dir");
    let target_base = resolve_target_dir(dir.path());
    let wasm_dir = target_base.join("wasm32-wasip1/release");
    std::fs::create_dir_all(&wasm_dir).expect("create wasm32-wasip1 dir");
    std::fs::File::create(wasm_dir.join("my_tool.wasm")).expect("create wasm artifact");

    let result = find_wasm_artifact(dir.path(), "my-tool", "release");
    assert!(result.is_some());
}

#[rstest]
fn test_find_wasm_artifact_prefers_wasip2_over_wasip1(_cleared_target_dir: ClearedTargetDirGuard) {
    let dir = TempDir::new().expect("temp dir");
    let target_base = resolve_target_dir(dir.path());
    let wasip1_dir = target_base.join("wasm32-wasip1/release");
    let wasip2_dir = target_base.join("wasm32-wasip2/release");
    std::fs::create_dir_all(&wasip1_dir).expect("create wasip1 dir");
    std::fs::create_dir_all(&wasip2_dir).expect("create wasip2 dir");
    std::fs::File::create(wasip1_dir.join("my_tool.wasm")).expect("create wasip1 wasm");
    std::fs::File::create(wasip2_dir.join("my_tool.wasm")).expect("create wasip2 wasm");

    let result =
        find_wasm_artifact(dir.path(), "my_tool", "release").expect("should find wasm artifact");
    assert!(
        result.ends_with("wasm32-wasip2/release/my_tool.wasm"),
        "expected wasm32-wasip2 artifact, got {}",
        result.display()
    );
}

#[rstest]
fn test_find_any_wasm_artifact_found(_cleared_target_dir: ClearedTargetDirGuard) {
    let dir = TempDir::new().expect("create temp dir");
    let target_base = resolve_target_dir(dir.path());
    let wasm_dir = target_base.join("wasm32-wasip2/release");
    std::fs::create_dir_all(&wasm_dir).expect("create wasm dir");
    std::fs::File::create(wasm_dir.join("something.wasm")).expect("create wasm artifact");

    let result = find_any_wasm_artifact(dir.path(), "release");
    assert!(result.is_some());
}

#[rstest]
fn test_find_any_wasm_artifact_not_found(_cleared_target_dir: ClearedTargetDirGuard) {
    let dir = TempDir::new().expect("create temp dir");
    assert!(find_any_wasm_artifact(dir.path(), "release").is_none());
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

    let result = install_wasm_files(
        &wasm_src,
        src_dir.path(),
        "mytool",
        target_dir.path(),
        false,
    )
    .await;

    assert!(result.is_ok());
    let wasm_dst = result.expect("install wasm files");
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
