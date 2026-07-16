//! Dev-mode tool loading: discover and load WASM tools directly from
//! build artefacts in `tools-src/`.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use tokio::fs;

use super::tool_loader::WasmToolLoader;
use super::{DiscoveredTool, LoadResults, WasmLoadError};

/// Compile-time project root, used to locate tools-src/ in dev builds.
const CARGO_MANIFEST_DIR: &str = env!("CARGO_MANIFEST_DIR");

/// Resolve the WASM target directory for a given crate directory.
///
/// Checks (in order):
/// 1. `CARGO_TARGET_DIR` env var (shared target dir)
/// 2. `<crate_dir>/target/` (default per-crate layout)
pub fn resolve_wasm_target_dir(crate_dir: &Path) -> PathBuf {
    crate::registry::artifacts::resolve_target_dir(crate_dir)
}

/// Return the expected path to a compiled WASM artefact for a given crate.
///
/// Combines [`resolve_wasm_target_dir`] with the `wasm32-wasip2/release/` subdirectory
/// and the binary name without extension (e.g. `slack_tool`).
///
/// `binary_name` should not include the `.wasm` extension; it is appended automatically.
///
/// This is a convenience function for callers that know the exact triple (wasip2)
/// and binary name. For multi-triple search, use
/// [`crate::registry::artifacts::find_wasm_artifact`] instead.
pub fn wasm_artifact_path(crate_dir: &Path, binary_name: &str) -> PathBuf {
    resolve_wasm_target_dir(crate_dir)
        .join("wasm32-wasip2/release")
        .join(format!("{}.wasm", binary_name))
}

/// Resolve the tools source directory.
///
/// Checks (in order):
/// 1. `IRONCLAW_TOOLS_SRC` env var
/// 2. `<CARGO_MANIFEST_DIR>/tools-src/` (dev builds)
pub(super) fn tools_src_dir() -> PathBuf {
    if let Ok(dir) = std::env::var("IRONCLAW_TOOLS_SRC") {
        return PathBuf::from(dir);
    }
    PathBuf::from(CARGO_MANIFEST_DIR).join("tools-src")
}

/// Discover WASM tools available as build artefacts in `tools-src/`.
///
/// Scans each subdirectory for:
/// - `tools-src/<name>/target/wasm32-wasip2/release/<crate_name>_tool.wasm`
/// - `tools-src/<name>/<name>-tool.capabilities.json`
///
/// Returns a map of install-name (e.g. "gmail-tool") to paths.
pub async fn discover_dev_tools() -> Result<HashMap<String, DiscoveredTool>, std::io::Error> {
    let src_dir = tools_src_dir();
    let mut tools = HashMap::new();

    if !src_dir.is_dir() {
        return Ok(tools);
    }

    let mut entries = fs::read_dir(&src_dir).await?;
    while let Some(entry) = entries.next_entry().await? {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }

        let dir_name = match path.file_name().and_then(|n| n.to_str()) {
            Some(n) => n.to_string(),
            None => continue,
        };

        // Convention: crate name uses underscores, directory uses hyphens
        let crate_name = dir_name.replace('-', "_");
        let install_name = format!("{}-tool", dir_name);

        let wasm_path = wasm_artifact_path(&path, &format!("{}_tool", crate_name));

        if !wasm_path.exists() {
            continue;
        }

        let caps_path = path.join(format!("{}-tool.capabilities.json", dir_name));

        tools.insert(
            install_name,
            DiscoveredTool {
                wasm_path,
                capabilities_path: if caps_path.exists() {
                    Some(caps_path)
                } else {
                    None
                },
            },
        );
    }

    Ok(tools)
}

/// Load WASM tools from build artefacts in `tools-src/`.
///
/// In dev mode, tools can be loaded directly from their build output without
/// needing to install them to `~/.ironclaw/tools/` first. Build artefacts
/// that are newer than installed copies take priority.
///
/// Set `IRONCLAW_TOOLS_SRC` env var to override the source directory.
pub async fn load_dev_tools(
    loader: &WasmToolLoader,
    install_dir: &Path,
) -> Result<LoadResults, WasmLoadError> {
    let dev_tools = discover_dev_tools().await?;
    let mut results = LoadResults::default();

    if dev_tools.is_empty() {
        return Ok(results);
    }

    for (name, discovered) in &dev_tools {
        // Check if the build artefact is newer than the installed copy
        let installed_path = install_dir.join(format!("{}.wasm", name));
        let should_load = if installed_path.exists() {
            // Compare modification times: prefer fresher build artefact
            match (
                fs::metadata(&discovered.wasm_path)
                    .await
                    .map(ambient_fs::Metadata::from_std),
                fs::metadata(&installed_path)
                    .await
                    .map(ambient_fs::Metadata::from_std),
            ) {
                (Ok(dev_meta), Ok(inst_meta)) => {
                    let dev_modified = dev_meta.modified().unwrap_or(std::time::UNIX_EPOCH);
                    let inst_modified = inst_meta.modified().unwrap_or(std::time::UNIX_EPOCH);
                    dev_modified > inst_modified
                }
                _ => true,
            }
        } else {
            true
        };

        if !should_load {
            continue;
        }

        tracing::info!(
            name = name,
            wasm_path = %discovered.wasm_path.display(),
            "Loading dev tool from build artefacts (newer than installed)"
        );

        match loader
            .load_from_files(
                name,
                &discovered.wasm_path,
                discovered.capabilities_path.as_deref(),
            )
            .await
        {
            Ok(()) => {
                results.loaded.push(name.clone());
            }
            Err(e) => {
                tracing::error!(
                    name = name,
                    error = %e,
                    "Failed to load dev tool"
                );
                results.errors.push((discovered.wasm_path.clone(), e));
            }
        }
    }

    if !results.loaded.is_empty() {
        tracing::info!(
            count = results.loaded.len(),
            tools = ?results.loaded,
            "Loaded dev tools from build artefacts"
        );
    }

    Ok(results)
}
