//! Listing, inspection, and removal helpers for installed tools.

use std::path::PathBuf;

use tokio::fs;

use crate::tools::wasm::{CapabilitiesFile, compute_binary_hash};

use super::default_tools_dir;
use super::printing::{print_capabilities_detail, print_capabilities_summary};

/// List installed tools.
pub(super) async fn list_tools(dir: Option<PathBuf>, verbose: bool) -> anyhow::Result<()> {
    let tools_dir = dir.unwrap_or_else(default_tools_dir);

    if !tools_dir.exists() {
        println!("No tools directory found at {}", tools_dir.display());
        println!("Install a tool with: ironclaw tool install <path>");
        return Ok(());
    }

    let mut entries = fs::read_dir(&tools_dir).await?;
    let mut tools = Vec::new();

    while let Some(entry) = entries.next_entry().await? {
        let path = entry.path();
        if path.extension().map(|e| e == "wasm").unwrap_or(false) {
            let name = path
                .file_stem()
                .and_then(|stem| stem.to_str())
                .unwrap_or("unknown")
                .to_string();

            let caps_path = path.with_extension("capabilities.json");
            let has_caps = caps_path.exists();
            let size = fs::metadata(&path)
                .await
                .map(|metadata| metadata.len())
                .unwrap_or(0);

            tools.push((name, path, has_caps, size));
        }
    }

    if tools.is_empty() {
        println!("No tools installed in {}", tools_dir.display());
        return Ok(());
    }

    tools.sort_by(|a, b| a.0.cmp(&b.0));

    println!("Installed tools in {}:", tools_dir.display());
    println!();

    for (name, path, has_caps, size) in tools {
        if verbose {
            let wasm_bytes = fs::read(&path).await?;
            let hash = compute_binary_hash(&wasm_bytes);
            let hash_hex: String = hash.iter().take(8).map(|b| format!("{:02x}", b)).collect();

            println!("  {} ({})", name, format_size(size));
            println!("    Path: {}", path.display());
            println!("    Hash: {}", hash_hex);
            println!("    Caps: {}", if has_caps { "yes" } else { "no" });

            if has_caps {
                let caps_path = path.with_extension("capabilities.json");
                if let Ok(content) = fs::read_to_string(&caps_path).await
                    && let Ok(caps) = CapabilitiesFile::from_json(&content)
                {
                    print_capabilities_summary(&caps);
                }
            }
            println!();
        } else {
            let caps_indicator = if has_caps { "✓" } else { "✗" };
            println!(
                "  {} ({}, caps: {})",
                name,
                format_size(size),
                caps_indicator
            );
        }
    }

    Ok(())
}

/// Remove an installed tool.
pub(super) async fn remove_tool(name: String, dir: Option<PathBuf>) -> anyhow::Result<()> {
    let tools_dir = dir.unwrap_or_else(default_tools_dir);

    let wasm_path = tools_dir.join(format!("{}.wasm", name));
    let caps_path = tools_dir.join(format!("{}.capabilities.json", name));

    if !wasm_path.exists() {
        anyhow::bail!("Tool '{}' not found in {}", name, tools_dir.display());
    }

    fs::remove_file(&wasm_path).await?;
    println!("Removed {}", wasm_path.display());

    if caps_path.exists() {
        fs::remove_file(&caps_path).await?;
        println!("Removed {}", caps_path.display());
    }

    println!("\nTool '{}' removed.", name);
    Ok(())
}

/// Show information about a tool.
pub(super) async fn show_tool_info(
    name_or_path: String,
    dir: Option<PathBuf>,
) -> anyhow::Result<()> {
    let wasm_path = if name_or_path.ends_with(".wasm") {
        PathBuf::from(&name_or_path)
    } else {
        let tools_dir = dir.unwrap_or_else(default_tools_dir);
        tools_dir.join(format!("{}.wasm", name_or_path))
    };

    if !wasm_path.exists() {
        anyhow::bail!("Tool not found: {}", wasm_path.display());
    }

    let wasm_bytes = fs::read(&wasm_path).await?;
    let hash = compute_binary_hash(&wasm_bytes);
    let hash_hex: String = hash.iter().map(|b| format!("{:02x}", b)).collect();

    let name = wasm_path
        .file_stem()
        .and_then(|stem| stem.to_str())
        .unwrap_or("unknown");

    println!("Tool: {}", name);
    println!("Path: {}", wasm_path.display());
    println!(
        "Size: {} bytes ({})",
        wasm_bytes.len(),
        format_size(wasm_bytes.len() as u64)
    );
    println!("Hash: {}", hash_hex);

    let caps_path = wasm_path.with_extension("capabilities.json");
    if caps_path.exists() {
        println!("\nCapabilities ({}):", caps_path.display());
        let content = fs::read_to_string(&caps_path).await?;
        match CapabilitiesFile::from_json(&content) {
            Ok(caps) => print_capabilities_detail(&caps),
            Err(e) => println!("  Error parsing: {}", e),
        }
    } else {
        println!("\nNo capabilities file found.");
        println!("Tool will have no permissions (default deny).");
    }

    Ok(())
}

/// Format bytes as human-readable size.
pub(super) fn format_size(bytes: u64) -> String {
    const KB: u64 = 1024;
    const MB: u64 = KB * 1024;

    if bytes >= MB {
        format!("{:.1} MB", bytes as f64 / MB as f64)
    } else if bytes >= KB {
        format!("{:.1} KB", bytes as f64 / KB as f64)
    } else {
        format!("{} B", bytes)
    }
}
