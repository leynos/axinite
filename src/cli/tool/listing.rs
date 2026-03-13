//! Listing, inspection, and removal helpers for installed tools.

use std::path::PathBuf;

use tokio::fs;

use crate::tools::wasm::{CapabilitiesFile, compute_binary_hash};

use super::default_tools_dir;
use super::printing::{print_capabilities_detail, print_capabilities_summary, validate_tool_name};

struct InstalledToolSummary {
    name: String,
    path: PathBuf,
    has_caps: bool,
    size: u64,
}

async fn scan_installed_tools(tools_dir: &PathBuf) -> anyhow::Result<Vec<InstalledToolSummary>> {
    let mut entries = fs::read_dir(tools_dir).await?;
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
            let has_caps = fs::try_exists(&caps_path).await?;
            let size = fs::metadata(&path).await?.len();
            tools.push(InstalledToolSummary {
                name,
                path,
                has_caps,
                size,
            });
        }
    }

    tools.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(tools)
}

async fn render_tool_summary(tool: &InstalledToolSummary, verbose: bool) -> anyhow::Result<()> {
    if verbose {
        let wasm_bytes = fs::read(&tool.path).await?;
        let hash = compute_binary_hash(&wasm_bytes);
        let hash_hex: String = hash.iter().take(8).map(|b| format!("{:02x}", b)).collect();

        println!("  {} ({})", tool.name, format_size(tool.size));
        println!("    Path: {}", tool.path.display());
        println!("    Hash: {}", hash_hex);
        println!("    Caps: {}", if tool.has_caps { "yes" } else { "no" });

        if tool.has_caps {
            let caps_path = tool.path.with_extension("capabilities.json");
            if let Ok(content) = fs::read_to_string(&caps_path).await
                && let Ok(caps) = CapabilitiesFile::from_json(&content)
            {
                print_capabilities_summary(&caps);
            }
        }
        println!();
    } else {
        let caps_indicator = if tool.has_caps { "✓" } else { "✗" };
        println!(
            "  {} ({}, caps: {})",
            tool.name,
            format_size(tool.size),
            caps_indicator
        );
    }

    Ok(())
}

/// List installed tools.
pub(super) async fn list_tools(dir: Option<PathBuf>, verbose: bool) -> anyhow::Result<()> {
    let tools_dir = dir.unwrap_or_else(default_tools_dir);

    if !fs::try_exists(&tools_dir).await? {
        println!("No tools directory found at {}", tools_dir.display());
        println!("Install a tool with: ironclaw tool install <path>");
        return Ok(());
    }

    let tools = scan_installed_tools(&tools_dir).await?;
    if tools.is_empty() {
        println!("No tools installed in {}", tools_dir.display());
        return Ok(());
    }

    println!("Installed tools in {}:", tools_dir.display());
    println!();

    for tool in &tools {
        render_tool_summary(tool, verbose).await?;
    }

    Ok(())
}

/// Remove an installed tool.
pub(super) async fn remove_tool(name: String, dir: Option<PathBuf>) -> anyhow::Result<()> {
    validate_tool_name(&name)?;
    let tools_dir = dir.unwrap_or_else(default_tools_dir);

    let wasm_path = tools_dir.join(format!("{}.wasm", name));
    let caps_path = tools_dir.join(format!("{}.capabilities.json", name));

    if !fs::try_exists(&wasm_path).await? {
        anyhow::bail!("Tool '{}' not found in {}", name, tools_dir.display());
    }

    fs::remove_file(&wasm_path).await?;
    println!("Removed {}", wasm_path.display());

    if fs::try_exists(&caps_path).await? {
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
        validate_tool_name(&name_or_path)?;
        let tools_dir = dir.unwrap_or_else(default_tools_dir);
        tools_dir.join(format!("{}.wasm", name_or_path))
    };

    if !fs::try_exists(&wasm_path).await? {
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
    if fs::try_exists(&caps_path).await? {
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
    let (value, unit) = size_components(bytes);
    match value {
        Some(value) => format!("{value:.1} {unit}"),
        None => format!("{bytes} {unit}"),
    }
}

fn size_components(bytes: u64) -> (Option<f64>, &'static str) {
    const KB: u64 = 1024;
    const MB: u64 = KB * 1024;

    if bytes >= MB {
        (Some(bytes as f64 / MB as f64), "MB")
    } else if bytes >= KB {
        (Some(bytes as f64 / KB as f64), "KB")
    } else {
        (None, "B")
    }
}
