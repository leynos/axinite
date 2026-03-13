//! Tool-installation helpers for the CLI.
//!
//! This module handles source-directory and standalone-WASM installs,
//! capabilities discovery, and crate-name extraction for the install command.

use std::path::{Path, PathBuf};

use tokio::fs;

use crate::tools::wasm::{CapabilitiesFile, compute_binary_hash};

use super::default_tools_dir;

async fn find_existing_path(candidates: Vec<PathBuf>) -> anyhow::Result<Option<PathBuf>> {
    for candidate in candidates {
        if fs::try_exists(&candidate).await? {
            return Ok(Some(candidate));
        }
    }
    Ok(None)
}

async fn resolve_directory_install(
    path: &Path,
    name: Option<String>,
    capabilities: Option<PathBuf>,
    release: bool,
    skip_build: bool,
) -> anyhow::Result<(PathBuf, String, Option<PathBuf>)> {
    let cargo_toml = path.join("Cargo.toml");
    if !fs::try_exists(&cargo_toml).await? {
        anyhow::bail!(
            "No Cargo.toml found in {}. Expected a Rust WASM tool source directory.",
            path.display()
        );
    }

    let tool_name = match name {
        Some(name) => name,
        None => extract_crate_name(&cargo_toml).await?,
    };
    let profile = if release { "release" } else { "debug" };
    let wasm_path = if skip_build {
        crate::registry::artifacts::find_wasm_artifact(path, &tool_name, profile)
            .or_else(|| crate::registry::artifacts::find_any_wasm_artifact(path, profile))
            .ok_or_else(|| {
                anyhow::anyhow!("No .wasm artifact found. Run without --skip-build to build first.")
            })?
    } else {
        let source_dir = path.to_path_buf();
        tokio::task::spawn_blocking(move || {
            crate::registry::artifacts::build_wasm_component_sync(&source_dir, release)
        })
        .await
        .map_err(|e| anyhow::anyhow!("tool build task failed: {e}"))??
    };
    let caps_path = match capabilities {
        Some(path) => Some(path),
        None => {
            find_existing_path(vec![
                path.join(format!("{tool_name}.capabilities.json")),
                path.join("capabilities.json"),
            ])
            .await?
        }
    };

    Ok((wasm_path, tool_name, caps_path))
}

async fn resolve_wasm_install(
    path: &Path,
    name: Option<String>,
    capabilities: Option<PathBuf>,
) -> anyhow::Result<(PathBuf, String, Option<PathBuf>)> {
    let tool_name = match name {
        Some(name) => name,
        None => path
            .file_stem()
            .and_then(|stem| stem.to_str())
            .unwrap_or("unknown")
            .to_string(),
    };
    let caps_path = match capabilities {
        Some(path) => Some(path),
        None => {
            let mut candidates = vec![path.with_extension("capabilities.json")];
            if let Some(parent) = path.parent() {
                candidates.push(parent.join(format!("{tool_name}.capabilities.json")));
            }
            find_existing_path(candidates).await?
        }
    };

    Ok((path.to_path_buf(), tool_name, caps_path))
}

/// Install a WASM tool.
pub(super) async fn install_tool(
    path: PathBuf,
    name: Option<String>,
    capabilities: Option<PathBuf>,
    target: Option<PathBuf>,
    release: bool,
    skip_build: bool,
    force: bool,
) -> anyhow::Result<()> {
    let target_dir = target.unwrap_or_else(default_tools_dir);

    let metadata = fs::metadata(&path).await?;

    let (wasm_path, tool_name, caps_path) = if metadata.is_dir() {
        resolve_directory_install(&path, name, capabilities, release, skip_build).await?
    } else if path.extension().map(|e| e == "wasm").unwrap_or(false) {
        resolve_wasm_install(&path, name, capabilities).await?
    } else {
        anyhow::bail!(
            "Expected a directory with Cargo.toml or a .wasm file, got: {}",
            path.display()
        );
    };

    fs::create_dir_all(&target_dir).await?;

    let target_wasm = target_dir.join(format!("{}.wasm", tool_name));
    let target_caps = target_dir.join(format!("{}.capabilities.json", tool_name));

    if fs::try_exists(&target_wasm).await? && !force {
        anyhow::bail!(
            "Tool '{}' already exists at {}. Use --force to overwrite.",
            tool_name,
            target_wasm.display()
        );
    }

    if let Some(ref caps) = caps_path {
        let content = fs::read_to_string(caps).await?;
        CapabilitiesFile::from_json(&content)
            .map_err(|e| anyhow::anyhow!("Invalid capabilities file {}: {}", caps.display(), e))?;
    }

    println!("Installing {} to {}", tool_name, target_wasm.display());
    fs::copy(&wasm_path, &target_wasm).await?;

    if let Some(caps) = caps_path {
        println!("  Copying capabilities from {}", caps.display());
        fs::copy(&caps, &target_caps).await?;
    } else {
        println!("  Warning: No capabilities file found. Tool will have no permissions.");
    }

    let wasm_bytes = fs::read(&target_wasm).await?;
    let hash = compute_binary_hash(&wasm_bytes);
    let hash_hex: String = hash.iter().map(|b| format!("{:02x}", b)).collect();

    println!("\nInstalled successfully:");
    println!("  Name: {}", tool_name);
    println!("  WASM: {}", target_wasm.display());
    println!("  Size: {} bytes", wasm_bytes.len());
    println!("  Hash: {}", &hash_hex[..16]);

    if fs::try_exists(&target_caps).await? {
        println!("  Caps: {}", target_caps.display());
    }

    Ok(())
}

/// Extract crate name from Cargo.toml.
pub(super) async fn extract_crate_name(cargo_toml: &Path) -> anyhow::Result<String> {
    let content = fs::read_to_string(cargo_toml).await?;
    let parsed: toml::Value = toml::from_str(&content)
        .map_err(|e| anyhow::anyhow!("Invalid Cargo.toml {}: {}", cargo_toml.display(), e))?;
    if let Some(name) = parsed
        .get("package")
        .and_then(|package| package.get("name"))
        .and_then(toml::Value::as_str)
    {
        return Ok(name.to_string());
    }

    anyhow::bail!(
        "Could not extract package name from {}",
        cargo_toml.display()
    )
}
