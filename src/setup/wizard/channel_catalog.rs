//! Discovery and installation helpers for WASM channels and the
//! extension registry catalog.

use super::*;

/// Discover WASM channels in a directory.
///
/// Returns a list of (channel_name, capabilities_file) pairs.
pub(super) async fn discover_wasm_channels(
    dir: &std::path::Path,
) -> Vec<(String, ChannelCapabilitiesFile)> {
    let mut channels = Vec::new();

    if !dir.is_dir() {
        return channels;
    }

    let mut entries = match tokio::fs::read_dir(dir).await {
        Ok(e) => e,
        Err(_) => return channels,
    };

    while let Ok(Some(entry)) = entries.next_entry().await {
        let path = entry.path();

        // Look for .capabilities.json files
        let filename = path.file_name().and_then(|n| n.to_str()).unwrap_or("");

        if !filename.ends_with(".capabilities.json") {
            continue;
        }

        // Extract channel name
        let name = filename.trim_end_matches(".capabilities.json").to_string();
        if name.is_empty() {
            continue;
        }

        // Check if corresponding .wasm file exists
        let wasm_path = dir.join(format!("{}.wasm", name));
        if !wasm_path.exists() {
            continue;
        }

        // Parse capabilities file
        match tokio::fs::read(&path).await {
            Ok(bytes) => match ChannelCapabilitiesFile::from_bytes(&bytes) {
                Ok(cap_file) => {
                    channels.push((name, cap_file));
                }
                Err(e) => {
                    tracing::warn!(
                        path = %path.display(),
                        error = %e,
                        "Failed to parse channel capabilities file"
                    );
                }
            },
            Err(e) => {
                tracing::warn!(
                    path = %path.display(),
                    error = %e,
                    "Failed to read channel capabilities file"
                );
            }
        }
    }

    // Sort by name for consistent ordering
    channels.sort_by(|a, b| a.0.cmp(&b.0));
    channels
}

#[cfg(test)]
pub(super) async fn install_missing_bundled_channels(
    channels_dir: &std::path::Path,
    already_installed: &HashSet<String>,
) -> Result<Vec<String>, SetupError> {
    let mut installed = Vec::new();

    for name in available_channel_names().iter().copied() {
        if already_installed.contains(name) {
            continue;
        }

        install_bundled_channel(name, channels_dir, false)
            .await
            .map_err(SetupError::Channel)?;
        installed.push(name.to_string());
    }

    Ok(installed)
}

/// Build channel options from discovered channels + bundled + registry catalog.
///
/// Returns a deduplicated, sorted list of channel names available for selection.
pub(super) fn build_channel_options(
    discovered: &[(String, ChannelCapabilitiesFile)],
) -> Vec<String> {
    let mut names: Vec<String> = discovered.iter().map(|(name, _)| name.clone()).collect();

    // Add bundled channels
    for bundled in available_channel_names().iter().copied() {
        if !names.iter().any(|name| name == bundled) {
            names.push(bundled.to_string());
        }
    }

    // Add registry channels
    if let Some(catalog) = load_registry_catalog() {
        for manifest in catalog.list(Some(crate::registry::manifest::ManifestKind::Channel), None) {
            if !names.iter().any(|n| n == &manifest.name) {
                names.push(manifest.name.clone());
            }
        }
    }

    names.sort();
    names
}

/// Try to load the registry catalog. Falls back to embedded manifests when
/// the `registry/` directory cannot be found (e.g. running from an installed binary).
pub(super) fn load_registry_catalog() -> Option<crate::registry::catalog::RegistryCatalog> {
    crate::registry::catalog::RegistryCatalog::load_or_embedded().ok()
}

/// Install selected channels from the registry that aren't already on disk
/// and weren't handled by the bundled installer.
pub(super) async fn install_selected_registry_channels(
    channels_dir: &std::path::Path,
    selected_channels: &[String],
    already_installed: &HashSet<String>,
) -> Vec<String> {
    let catalog = match load_registry_catalog() {
        Some(c) => c,
        None => return Vec::new(),
    };

    let repo_root = catalog
        .root()
        .parent()
        .unwrap_or(catalog.root())
        .to_path_buf();

    let bundled: HashSet<&str> = available_channel_names().iter().copied().collect();
    let mut installed = Vec::new();

    for name in selected_channels {
        // Skip if already installed or handled by bundled installer
        if already_installed.contains(name) || bundled.contains(name.as_str()) {
            continue;
        }

        // Check if already on disk (may have been installed between bundled and here)
        let wasm_on_disk = channels_dir.join(format!("{}.wasm", name)).exists()
            || channels_dir.join(format!("{}-channel.wasm", name)).exists();
        if wasm_on_disk {
            continue;
        }

        // Look up in registry
        let manifest = match catalog.get(&format!("channels/{}", name)) {
            Some(m) => m,
            None => continue,
        };

        let installer = crate::registry::installer::RegistryInstaller::new(
            repo_root.clone(),
            ironclaw_base_dir().join("tools"),
            channels_dir.to_path_buf(),
        );

        match installer
            .install_with_source_fallback(manifest, false)
            .await
        {
            Ok(outcome) => {
                for warning in &outcome.warnings {
                    crate::setup::prompts::print_info(&format!("{}: {}", name, warning));
                }
                installed.push(name.clone());
            }
            Err(e) => {
                tracing::warn!(
                    channel = %name,
                    error = %e,
                    "Failed to install channel from registry"
                );
                crate::setup::prompts::print_error(&format!(
                    "Failed to install channel '{}': {}",
                    name, e
                ));
            }
        }
    }

    installed
}

/// Discover which tools are already installed in the tools directory.
///
/// Returns a set of tool names (the stem of .wasm files).
pub(super) async fn discover_installed_tools(tools_dir: &std::path::Path) -> HashSet<String> {
    let mut names = HashSet::new();

    if !tools_dir.is_dir() {
        return names;
    }

    let mut entries = match tokio::fs::read_dir(tools_dir).await {
        Ok(e) => e,
        Err(_) => return names,
    };

    while let Ok(Some(entry)) = entries.next_entry().await {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) == Some("wasm")
            && let Some(stem) = path.file_stem().and_then(|s| s.to_str())
        {
            names.insert(stem.to_string());
        }
    }

    names
}

pub(super) async fn install_selected_bundled_channels(
    channels_dir: &std::path::Path,
    selected_channels: &[String],
    already_installed: &HashSet<String>,
) -> Result<Option<Vec<String>>, SetupError> {
    let bundled: HashSet<&str> = available_channel_names().iter().copied().collect();
    let selected_missing: HashSet<String> = selected_channels
        .iter()
        .filter(|name| bundled.contains(name.as_str()) && !already_installed.contains(*name))
        .cloned()
        .collect();

    if selected_missing.is_empty() {
        return Ok(None);
    }

    let mut installed = Vec::new();
    for name in selected_missing {
        install_bundled_channel(&name, channels_dir, false)
            .await
            .map_err(SetupError::Channel)?;
        installed.push(name);
    }

    installed.sort();
    Ok(Some(installed))
}
