//! Unified WASM artifact resolution: find, build, and install WASM components.
//!
//! This module consolidates all WASM artifact logic that was previously duplicated
//! across `cli/tool.rs`, `registry/installer.rs`, `extensions/manager.rs`,
//! `channels/wasm/bundled.rs`, and `tools/wasm/loader.rs`.
//!
//! # Functions
//!
//! - [`resolve_target_dir`] — resolve the cargo target directory for a crate
//! - [`find_wasm_artifact`] — find a compiled `.wasm` by crate name across all triples
//! - [`find_any_wasm_artifact`] — find any `.wasm` file (fallback when name is unknown)
//! - [`build_wasm_component`] — async build via `cargo component build`
//! - [`build_wasm_component_sync`] — sync build for CLI use
//! - [`install_wasm_files`] — copy `.wasm` + optional `.capabilities.json` to install dir

use std::path::{Path, PathBuf};

use tokio::fs;

/// WASM target triples to search, in priority order.
const WASM_TRIPLES: &[&str] = &[
    "wasm32-wasip2",
    "wasm32-wasip1",
    "wasm32-wasi",
    "wasm32-unknown-unknown",
];

const SHARED_WASM_TARGET_DIR: &str = "target/wasm-extensions";
/// Resolve the cargo target directory for a crate.
///
/// Checks (in order):
/// 1. `CARGO_TARGET_DIR` env var (shared target dir)
/// 2. `<crate_dir>/target/` (default per-crate layout)
pub fn resolve_target_dir(crate_dir: &Path) -> PathBuf {
    if let Some(dir) = resolve_env_target_dir() {
        return dir;
    }
    crate_dir.join("target")
}

/// Find a compiled WASM artifact by searching across all target triples.
///
/// Tries exact name match first (with hyphen-to-underscore normalization),
/// then falls back to searching in whichever target directory exists.
/// `profile` is `"release"` or `"debug"`.
pub fn find_wasm_artifact(crate_dir: &Path, crate_name: &str, profile: &str) -> Option<PathBuf> {
    let snake_name = crate_name.replace('-', "_");

    for target_base in candidate_target_dirs(crate_dir) {
        // Try exact name match in each target triple directory
        for triple in WASM_TRIPLES {
            let dir = target_base.join(triple).join(profile);
            let candidates = [
                dir.join(format!("{}.wasm", crate_name)),
                dir.join(format!("{}.wasm", snake_name)),
            ];
            for candidate in &candidates {
                if candidate.exists() {
                    return Some(candidate.clone());
                }
            }
        }
    }

    None
}

/// Find any `.wasm` file in the target dirs (fallback when crate name is unknown).
///
/// Returns the first `.wasm` found across target triples.
pub fn find_any_wasm_artifact(crate_dir: &Path, profile: &str) -> Option<PathBuf> {
    for target_base in candidate_target_dirs(crate_dir) {
        for triple in WASM_TRIPLES {
            let dir = target_base.join(triple).join(profile);
            if !dir.is_dir() {
                continue;
            }
            if let Ok(entries) = std::fs::read_dir(&dir) {
                for entry in entries.flatten() {
                    let path = entry.path();
                    if path.extension().map(|ext| ext == "wasm").unwrap_or(false) {
                        return Some(path);
                    }
                }
            }
        }
    }

    None
}

/// Build a WASM component using `cargo-component` (async).
///
/// Streams build output to the terminal. Returns the path to the built artifact.
pub async fn build_wasm_component(
    source_dir: &Path,
    crate_name: &str,
    release: bool,
) -> anyhow::Result<PathBuf> {
    use tokio::process::Command;

    // Check cargo-component availability
    let check = Command::new("cargo")
        .args(["component", "--version"])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .await;

    if check.is_err() || !check.as_ref().map(|s| s.success()).unwrap_or(false) {
        anyhow::bail!("cargo-component not found. Install with: cargo install cargo-component");
    }

    let mut cmd = Command::new("cargo");
    cmd.current_dir(source_dir).args(["component", "build"]);

    if release {
        cmd.arg("--release");
    }

    // Use status() with inherited stdio so build output streams to the terminal.
    let status = cmd.status().await?;

    if !status.success() {
        anyhow::bail!("Build failed (exit code: {})", status);
    }

    let profile = if release { "release" } else { "debug" };
    let wasm_filename = format!("{}.wasm", crate_name.replace('-', "_"));

    // Look for the specific crate's WASM file across target triples
    find_wasm_artifact(source_dir, wasm_filename.trim_end_matches(".wasm"), profile)
        .or_else(|| {
            // Fall back: search by crate_name directly
            find_wasm_artifact(source_dir, crate_name, profile)
        })
        .or_else(|| find_any_wasm_artifact(source_dir, profile))
        .ok_or_else(|| {
            anyhow::anyhow!(
                "Could not find {} in {}/target/*/{}/ after build",
                wasm_filename,
                source_dir.display(),
                profile,
            )
        })
}

/// Build a WASM component using `cargo-component` (sync, for CLI use).
///
/// Returns the path to the built artifact.
pub fn build_wasm_component_sync(source_dir: &Path, release: bool) -> anyhow::Result<PathBuf> {
    use std::process::Command;

    println!("Building WASM component in {}...", source_dir.display());

    // Check if cargo-component is available
    let check = Command::new("cargo")
        .args(["component", "--version"])
        .output();

    if check.is_err() || !check.as_ref().map(|o| o.status.success()).unwrap_or(false) {
        anyhow::bail!(
            "cargo-component not found. Install with: cargo install cargo-component\n\
             Or use --skip-build with an existing .wasm file."
        );
    }

    let mut cmd = Command::new("cargo");
    cmd.current_dir(source_dir).args(["component", "build"]);

    if release {
        cmd.arg("--release");
    }

    println!(
        "  Running: cargo component build{}",
        if release { " --release" } else { "" }
    );

    let output = cmd.output()?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("Build failed:\n{}", stderr);
    }

    let profile = if release { "release" } else { "debug" };

    // Find the built artifact
    find_any_wasm_artifact(source_dir, profile).ok_or_else(|| {
        anyhow::anyhow!(
            "No .wasm file found after build in {}/target/*/{}",
            source_dir.display(),
            profile,
        )
    })
}

/// Copy WASM binary + optional `capabilities.json` sidecar to an install directory.
///
/// Looks for capabilities files in `source_dir` matching several naming conventions.
/// Returns the destination wasm path.
pub async fn install_wasm_files(
    wasm_src: &Path,
    source_dir: &Path,
    name: &str,
    target_dir: &Path,
    force: bool,
) -> anyhow::Result<PathBuf> {
    fs::create_dir_all(target_dir).await?;

    let wasm_dst = target_dir.join(format!("{}.wasm", name));
    let caps_dst = target_dir.join(format!("{}.capabilities.json", name));

    if wasm_dst.exists() && !force {
        anyhow::bail!(
            "Tool '{}' already exists at {}. Use --force to overwrite.",
            name,
            wasm_dst.display()
        );
    }

    // Copy WASM binary
    fs::copy(wasm_src, &wasm_dst).await?;

    // Look for capabilities.json sidecar in the source directory
    let caps_candidates = [
        source_dir.join(format!("{}.capabilities.json", name)),
        source_dir.join(format!("{}-tool.capabilities.json", name)),
        source_dir.join("capabilities.json"),
    ];
    for caps_src in &caps_candidates {
        if caps_src.exists() {
            if let Err(e) = fs::copy(caps_src, &caps_dst).await {
                tracing::warn!(
                    "Failed to copy capabilities sidecar {} -> {}: {}",
                    caps_src.display(),
                    caps_dst.display(),
                    e,
                );
            }
            break;
        }
    }

    Ok(wasm_dst)
}

#[cfg(test)]
mod tests {
    use crate::testing::test_utils::EnvVarsGuard;
    use tempfile::TempDir;

    use super::*;

    fn with_cleared_target_dir<T>(f: impl FnOnce() -> T) -> T {
        let guard = EnvVarsGuard::new(&["CARGO_TARGET_DIR"]);
        guard.remove("CARGO_TARGET_DIR");
        f()
    }

    #[test]
    fn test_resolve_target_dir_default() {
        // When CARGO_TARGET_DIR is not set, should return <crate_dir>/target
        with_cleared_target_dir(|| {
            let dir = Path::new("/some/crate");
            let result = resolve_target_dir(dir);
            assert_eq!(result, dir.join("target"));
        });
    }

    #[test]
    fn test_find_wasm_artifact_falls_back_to_repo_shared_target_dir() {
        with_cleared_target_dir(|| {
            let repo = TempDir::new().expect("create temp dir");
            let crate_dir = repo.path().join("channels-src/demo");
            let shared_target = repo.path().join(SHARED_WASM_TARGET_DIR);
            let wasm_dir = shared_target.join("wasm32-wasip2/release");

            std::fs::create_dir_all(&crate_dir).expect("create crate dir");
            std::fs::create_dir_all(&wasm_dir).expect("create shared wasm dir");
            std::fs::File::create(wasm_dir.join("demo_channel.wasm"))
                .expect("create shared wasm artifact");

            let result = find_wasm_artifact(&crate_dir, "demo-channel", "release");
            assert_eq!(
                result.expect("find demo-channel artifact"),
                wasm_dir.join("demo_channel.wasm")
            );
        });
    }

    #[test]
    fn test_find_wasm_artifact_not_found() {
        let dir = TempDir::new().expect("create temp dir");
        assert!(find_wasm_artifact(dir.path(), "nonexistent", "release").is_none());
    }

    #[test]
    fn test_find_wasm_artifact_found() {
        with_cleared_target_dir(|| {
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
        });
    }

    #[test]
    fn test_find_wasm_artifact_hyphen_to_underscore() {
        with_cleared_target_dir(|| {
            let dir = TempDir::new().expect("create temp dir");
            let target_base = resolve_target_dir(dir.path());
            let wasm_dir = target_base.join("wasm32-wasip1/release");
            std::fs::create_dir_all(&wasm_dir).expect("create wasm32-wasip1 dir");
            std::fs::File::create(wasm_dir.join("my_tool.wasm")).expect("create wasm artifact");

            // Search with hyphens, should find underscore version
            let result = find_wasm_artifact(dir.path(), "my-tool", "release");
            assert!(result.is_some());
        });
    }

    #[test]
    fn test_find_wasm_artifact_prefers_wasip2_over_wasip1() {
        with_cleared_target_dir(|| {
            let dir = TempDir::new().expect("temp dir");
            let target_base = resolve_target_dir(dir.path());
            let wasip1_dir = target_base.join("wasm32-wasip1/release");
            let wasip2_dir = target_base.join("wasm32-wasip2/release");
            std::fs::create_dir_all(&wasip1_dir).expect("create wasip1 dir");
            std::fs::create_dir_all(&wasip2_dir).expect("create wasip2 dir");
            std::fs::File::create(wasip1_dir.join("my_tool.wasm")).expect("create wasip1 wasm");
            std::fs::File::create(wasip2_dir.join("my_tool.wasm")).expect("create wasip2 wasm");

            let result = find_wasm_artifact(dir.path(), "my_tool", "release")
                .expect("should find wasm artifact");
            assert!(
                result.ends_with("wasm32-wasip2/release/my_tool.wasm"),
                "expected wasm32-wasip2 artifact, got {}",
                result.display()
            );
        });
    }

    #[test]
    fn test_find_any_wasm_artifact_found() {
        with_cleared_target_dir(|| {
            let dir = TempDir::new().expect("create temp dir");
            let target_base = resolve_target_dir(dir.path());
            let wasm_dir = target_base.join("wasm32-wasip2/release");
            std::fs::create_dir_all(&wasm_dir).expect("create wasm dir");
            std::fs::File::create(wasm_dir.join("something.wasm")).expect("create wasm artifact");

            let result = find_any_wasm_artifact(dir.path(), "release");
            assert!(result.is_some());
        });
    }

    #[test]
    fn test_find_any_wasm_artifact_not_found() {
        with_cleared_target_dir(|| {
            let dir = TempDir::new().expect("create temp dir");
            assert!(find_any_wasm_artifact(dir.path(), "release").is_none());
        });
    }

    #[tokio::test]
    async fn test_install_wasm_files_copies() {
        let src_dir = TempDir::new().expect("create source temp dir");
        let target_dir = TempDir::new().expect("create target temp dir");

        let wasm_src = src_dir.path().join("test.wasm");
        tokio::fs::write(&wasm_src, b"\0asm\x01\x00\x00\x00")
            .await
            .expect("write source wasm");

        // Create a capabilities file
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

        // Pre-create the target
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
        // Verify the order is as documented
        assert_eq!(WASM_TRIPLES[0], "wasm32-wasip2");
        assert_eq!(WASM_TRIPLES[1], "wasm32-wasip1");
        assert_eq!(WASM_TRIPLES[2], "wasm32-wasi");
        assert_eq!(WASM_TRIPLES[3], "wasm32-unknown-unknown");
    }
}

fn resolve_env_target_dir() -> Option<PathBuf> {
    let dir = std::env::var("CARGO_TARGET_DIR").ok()?;
    let p = PathBuf::from(dir);
    if p.is_relative()
        && let Ok(cwd) = std::env::current_dir()
    {
        return Some(cwd.join(p));
    }
    Some(p)
}

fn candidate_target_dirs(crate_dir: &Path) -> Vec<PathBuf> {
    let mut candidates = Vec::new();

    if let Some(dir) = resolve_env_target_dir() {
        candidates.push(dir);
    }

    candidates.push(crate_dir.join("target"));

    if let Some(dir) = repo_shared_target_dir(crate_dir)
        && !candidates.iter().any(|candidate| candidate == &dir)
    {
        candidates.push(dir);
    }

    candidates
}

fn repo_shared_target_dir(crate_dir: &Path) -> Option<PathBuf> {
    let source_root = crate_dir.parent()?;
    let source_root_name = source_root.file_name()?.to_str()?;
    if !matches!(source_root_name, "channels-src" | "tools-src") {
        return None;
    }

    let repo_root = source_root.parent()?;
    let shared = repo_root.join(SHARED_WASM_TARGET_DIR);
    shared.exists().then_some(shared)
}
