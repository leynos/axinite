use std::path::{Path, PathBuf};

use tokio::fs;

use crate::tools::wasm::{CapabilitiesFile, compute_binary_hash};

use super::default_tools_dir;

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
        let cargo_toml = path.join("Cargo.toml");
        if !cargo_toml.exists() {
            anyhow::bail!(
                "No Cargo.toml found in {}. Expected a Rust WASM tool source directory.",
                path.display()
            );
        }

        let tool_name = if let Some(n) = name {
            n
        } else {
            extract_crate_name(&cargo_toml).await?
        };

        let profile = if release { "release" } else { "debug" };
        let wasm_path = if skip_build {
            crate::registry::artifacts::find_wasm_artifact(&path, &tool_name, profile)
                .or_else(|| crate::registry::artifacts::find_any_wasm_artifact(&path, profile))
                .ok_or_else(|| {
                    anyhow::anyhow!(
                        "No .wasm artifact found. Run without --skip-build to build first."
                    )
                })?
        } else {
            crate::registry::artifacts::build_wasm_component_sync(&path, release)?
        };

        let caps_path = capabilities.or_else(|| {
            let candidates = [
                path.join(format!("{}.capabilities.json", tool_name)),
                path.join("capabilities.json"),
            ];
            candidates.into_iter().find(|candidate| candidate.exists())
        });

        (wasm_path, tool_name, caps_path)
    } else if path.extension().map(|e| e == "wasm").unwrap_or(false) {
        let tool_name = name.unwrap_or_else(|| {
            path.file_stem()
                .and_then(|stem| stem.to_str())
                .unwrap_or("unknown")
                .to_string()
        });

        let caps_path = capabilities.or_else(|| {
            let candidates = [
                path.with_extension("capabilities.json"),
                path.parent()
                    .map(|parent| parent.join(format!("{}.capabilities.json", tool_name)))
                    .unwrap_or_default(),
            ];
            candidates.into_iter().find(|candidate| candidate.exists())
        });

        (path, tool_name, caps_path)
    } else {
        anyhow::bail!(
            "Expected a directory with Cargo.toml or a .wasm file, got: {}",
            path.display()
        );
    };

    fs::create_dir_all(&target_dir).await?;

    let target_wasm = target_dir.join(format!("{}.wasm", tool_name));
    let target_caps = target_dir.join(format!("{}.capabilities.json", tool_name));

    if target_wasm.exists() && !force {
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

    if target_caps.exists() {
        println!("  Caps: {}", target_caps.display());
    }

    Ok(())
}

/// Extract crate name from Cargo.toml.
pub(super) async fn extract_crate_name(cargo_toml: &Path) -> anyhow::Result<String> {
    let content = fs::read_to_string(cargo_toml).await?;

    for line in content.lines() {
        let line = line.trim();
        if line.starts_with("name")
            && let Some((_, value)) = line.split_once('=')
        {
            let name = value.trim().trim_matches('"').trim_matches('\'');
            return Ok(name.to_string());
        }
    }

    anyhow::bail!(
        "Could not extract package name from {}",
        cargo_toml.display()
    )
}
