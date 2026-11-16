//! Build script: embed registry manifests for the host binary.

#![forbid(unsafe_code)]

use std::env;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

/// Catalogue written when the registry directory is absent.
const EMPTY_CATALOGUE: &str = r#"{"tools":[],"channels":[],"bundles":{"bundles":{}}}"#;

/// Bundles section used when `registry/_bundles.json` is absent.
const DEFAULT_BUNDLES: &str = r#"{"bundles":{}}"#;

#[expect(
    clippy::disallowed_methods,
    reason = "owning Cargo build-script boundary; Cargo supplies build inputs through its environment"
)]
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let manifest_dir = env::var("CARGO_MANIFEST_DIR")?;
    let root = PathBuf::from(&manifest_dir);

    // ── Embed registry manifests ────────────────────────────────────────
    embed_registry_catalogue(&root)?;
    Ok(())
}

/// Collect all registry manifests into a single JSON blob at compile time.
///
/// Output: `$OUT_DIR/embedded_catalog.json` with structure:
/// ```json
/// { "tools": [...], "channels": [...], "bundles": {...} }
/// ```
fn embed_registry_catalogue(root: &Path) -> Result<(), Box<dyn std::error::Error>> {
    let registry_dir = root.join("registry");

    // Always watch the registry paths themselves so Cargo reruns when the
    // directories appear later; collect_json_files adds per-file watches for
    // existing manifests to track content changes reliably.
    announce_registry_watches();

    let out_path = embedded_catalogue_path()?;
    let catalogue = catalogue_for(root, &registry_dir)?;
    fs::write(&out_path, catalogue)?;
    Ok(())
}

/// Announce the registry paths Cargo must watch for later appearance.
fn announce_registry_watches() {
    for path in [
        "registry",
        "registry/tools",
        "registry/channels",
        "registry/_bundles.json",
    ] {
        println!("cargo:rerun-if-changed={path}");
    }
}

/// Resolve the catalogue output path from Cargo's `OUT_DIR`.
#[expect(
    clippy::disallowed_methods,
    reason = "owning Cargo build-script boundary; Cargo supplies build outputs through its environment"
)]
fn embedded_catalogue_path() -> Result<PathBuf, Box<dyn std::error::Error>> {
    let out_dir = PathBuf::from(env::var("OUT_DIR")?);
    Ok(out_dir.join("embedded_catalogue.json"))
}

/// Build the catalogue, or return the empty form when the registry is absent.
fn catalogue_for(root: &Path, registry_dir: &Path) -> Result<String, Box<dyn std::error::Error>> {
    if !registry_dir.is_dir() {
        return Ok(EMPTY_CATALOGUE.to_string());
    }
    build_catalogue(root, registry_dir)
}

/// Combine the tool manifests, channel manifests and bundle definitions.
fn build_catalogue(root: &Path, registry_dir: &Path) -> Result<String, Box<dyn std::error::Error>> {
    let mut tools = Vec::new();
    collect_manifests(root, &registry_dir.join("tools"), &mut tools)?;

    let mut channels = Vec::new();
    collect_manifests(root, &registry_dir.join("channels"), &mut channels)?;

    let bundles_raw = read_bundles(&registry_dir.join("_bundles.json"))?;
    Ok(format!(
        r#"{{"tools":[{}],"channels":[{}],"bundles":{}}}"#,
        tools.join(","),
        channels.join(","),
        bundles_raw,
    ))
}

/// Collect manifests from `dir` when that optional directory exists.
fn collect_manifests(
    root: &Path,
    dir: &Path,
    out: &mut Vec<String>,
) -> Result<(), Box<dyn std::error::Error>> {
    if dir.is_dir() {
        collect_json_files(root, dir, out)?;
    }
    Ok(())
}

/// Read the bundle definitions, falling back to an empty bundle set.
fn read_bundles(path: &Path) -> Result<String, Box<dyn std::error::Error>> {
    if !path.is_file() {
        return Ok(DEFAULT_BUNDLES.to_string());
    }
    let raw = fs::read_to_string(path)
        .map_err(|error| io::Error::other(format!("failed to read {}: {error}", path.display())))?;
    Ok(raw)
}

/// Read all `.json` files from `dir` and push their raw contents into `out`.
///
/// `collect_json_files(root, dir, out)` emits `cargo:rerun-if-changed` entries
/// relative to `root`, not `dir`, and normalizes those emitted paths to use
/// forward slashes.
fn collect_json_files(
    root: &Path,
    dir: &Path,
    out: &mut Vec<String>,
) -> Result<(), Box<dyn std::error::Error>> {
    for path in sorted_json_paths(dir)? {
        out.push(watched_contents(root, &path)?);
    }
    Ok(())
}

/// List the JSON files in `dir`, sorted by file name for deterministic output.
fn sorted_json_paths(dir: &Path) -> Result<Vec<PathBuf>, Box<dyn std::error::Error>> {
    let mut entries: Vec<_> = fs::read_dir(dir)
        .map_err(|error| io::Error::other(format!("failed to read {}: {error}", dir.display())))?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| io::Error::other(format!("failed to list {}: {error}", dir.display())))?;

    entries.retain(is_json_manifest);
    entries.sort_by_key(|entry| entry.file_name());
    Ok(entries.into_iter().map(|entry| entry.path()).collect())
}

/// Report whether a directory entry is a regular file with a `.json` suffix.
fn is_json_manifest(entry: &fs::DirEntry) -> bool {
    entry.path().is_file() && entry.path().extension().and_then(|ext| ext.to_str()) == Some("json")
}

/// Watch `path` relative to `root` and return its contents.
fn watched_contents(root: &Path, path: &Path) -> Result<String, Box<dyn std::error::Error>> {
    let relative_path = path.strip_prefix(root).map_err(|error| {
        io::Error::other(format!(
            "failed to strip {} from {}: {error}",
            root.display(),
            path.display()
        ))
    })?;

    // Emit per-file watch so Cargo reruns when file contents change.
    println!(
        "cargo:rerun-if-changed={}",
        relative_path.to_string_lossy().replace('\\', "/")
    );

    let content = fs::read_to_string(path)
        .map_err(|error| io::Error::other(format!("failed to read {}: {error}", path.display())))?;
    Ok(content)
}
