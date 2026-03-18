//! Build script: embed registry manifests for the host binary.

use std::env;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let manifest_dir = env::var("CARGO_MANIFEST_DIR")?;
    let root = PathBuf::from(&manifest_dir);

    // ── Embed registry manifests ────────────────────────────────────────
    embed_registry_catalog(&root)?;
    Ok(())
}

/// Collect all registry manifests into a single JSON blob at compile time.
///
/// Output: `$OUT_DIR/embedded_catalog.json` with structure:
/// ```json
/// { "tools": [...], "channels": [...], "bundles": {...} }
/// ```
fn embed_registry_catalog(root: &Path) -> Result<(), Box<dyn std::error::Error>> {
    let registry_dir = root.join("registry");

    // Watch the registry roots even if some subdirectories are absent on the
    // first build, then add per-file watches for present manifests.
    println!("cargo:rerun-if-changed=registry");
    println!("cargo:rerun-if-changed=registry/tools");
    println!("cargo:rerun-if-changed=registry/channels");
    println!("cargo:rerun-if-changed=registry/_bundles.json");

    let out_dir = PathBuf::from(env::var("OUT_DIR")?);
    let out_path = out_dir.join("embedded_catalog.json");

    if !registry_dir.is_dir() {
        // No registry dir: write empty catalog
        fs::write(
            &out_path,
            r#"{"tools":[],"channels":[],"bundles":{"bundles":{}}}"#,
        )?;
        return Ok(());
    }

    let mut tools = Vec::new();
    let mut channels = Vec::new();

    // Collect tool manifests
    let tools_dir = registry_dir.join("tools");
    if tools_dir.is_dir() {
        collect_json_files(&tools_dir, &mut tools)?;
    }

    // Collect channel manifests
    let channels_dir = registry_dir.join("channels");
    if channels_dir.is_dir() {
        collect_json_files(&channels_dir, &mut channels)?;
    }

    // Read bundles
    let bundles_path = registry_dir.join("_bundles.json");
    let bundles_raw = if bundles_path.is_file() {
        fs::read_to_string(&bundles_path).map_err(|e| {
            io::Error::other(format!("failed to read {}: {e}", bundles_path.display()))
        })?
    } else {
        r#"{"bundles":{}}"#.to_string()
    };

    // Build the combined JSON
    let catalog = format!(
        r#"{{"tools":[{}],"channels":[{}],"bundles":{}}}"#,
        tools.join(","),
        channels.join(","),
        bundles_raw,
    );

    fs::write(&out_path, catalog)?;
    Ok(())
}

/// Read all .json files from a directory and push their raw contents into `out`.
fn collect_json_files(dir: &Path, out: &mut Vec<String>) -> Result<(), Box<dyn std::error::Error>> {
    let mut entries: Vec<_> = fs::read_dir(dir)
        .map_err(|e| io::Error::other(format!("failed to read {}: {e}", dir.display())))?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| io::Error::other(format!("failed to list {}: {e}", dir.display())))?
        .into_iter()
        .filter(|e| {
            e.path().is_file() && e.path().extension().and_then(|x| x.to_str()) == Some("json")
        })
        .collect();

    // Sort for deterministic output
    entries.sort_by_key(|e| e.file_name());

    for entry in entries {
        // Emit per-file watch so Cargo reruns when file contents change
        println!("cargo:rerun-if-changed={}", entry.path().display());
        let path = entry.path();
        let content = fs::read_to_string(&path)
            .map_err(|e| io::Error::other(format!("failed to read {}: {e}", path.display())))?;
        out.push(content);
    }

    Ok(())
}
