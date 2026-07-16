//! Discovery and installation helpers for WASM channels and the
//! extension registry catalogue.

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
        if let Some(channel) = load_channel_entry(dir, &entry.path()).await {
            channels.push(channel);
        }
    }

    // Sort by name for consistent ordering
    channels.sort_by(|a, b| a.0.cmp(&b.0));
    channels
}

/// Extract the channel name from a `.capabilities.json` path when the
/// matching `.wasm` artefact exists alongside it.
fn channel_name_for_capabilities(dir: &std::path::Path, path: &std::path::Path) -> Option<String> {
    let filename = path.file_name().and_then(|n| n.to_str())?;
    let name = filename.strip_suffix(".capabilities.json")?;
    if name.is_empty() {
        return None;
    }
    let wasm_path = dir.join(format!("{}.wasm", name));
    if !wasm_path.exists() {
        return None;
    }
    Some(name.to_string())
}

/// Load one directory entry as a channel, returning `None` (with a warning
/// where appropriate) when the entry is not a valid channel.
async fn load_channel_entry(
    dir: &std::path::Path,
    path: &std::path::Path,
) -> Option<(String, ChannelCapabilitiesFile)> {
    let name = channel_name_for_capabilities(dir, path)?;

    let bytes = match tokio::fs::read(path).await {
        Ok(bytes) => bytes,
        Err(e) => {
            tracing::warn!(
                path = %path.display(),
                error = %e,
                "Failed to read channel capabilities file"
            );
            return None;
        }
    };

    match ChannelCapabilitiesFile::from_bytes(&bytes) {
        Ok(cap_file) => Some((name, cap_file)),
        Err(e) => {
            tracing::warn!(
                path = %path.display(),
                error = %e,
                "Failed to parse channel capabilities file"
            );
            None
        }
    }
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

/// Build channel options from discovered channels + bundled + registry catalogue.
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
    if let Some(catalogue) = load_registry_catalogue() {
        for manifest in catalogue.list(Some(crate::registry::manifest::ManifestKind::Channel), None)
        {
            if !names.iter().any(|n| n == &manifest.name) {
                names.push(manifest.name.clone());
            }
        }
    }

    names.sort();
    names
}

/// Try to load the registry catalogue. Falls back to embedded manifests when
/// the `registry/` directory cannot be found (e.g. running from an installed binary).
pub(super) fn load_registry_catalogue() -> Option<crate::registry::catalog::RegistryCatalog> {
    crate::registry::catalog::RegistryCatalog::load_or_embedded().ok()
}

/// Install selected channels from the registry that aren't already on disk
/// and weren't handled by the bundled installer.
pub(super) async fn install_selected_registry_channels(
    channels_dir: &std::path::Path,
    selected_channels: &[String],
    already_installed: &HashSet<String>,
) -> Vec<String> {
    let catalogue = match load_registry_catalogue() {
        Some(c) => c,
        None => return Vec::new(),
    };

    let repo_root = catalogue
        .root()
        .parent()
        .unwrap_or(catalogue.root())
        .to_path_buf();

    let bundled: HashSet<&str> = available_channel_names().iter().copied().collect();
    let mut installed = Vec::new();

    for name in selected_channels {
        // Skip if already installed or handled by bundled installer
        if already_installed.contains(name) || bundled.contains(name.as_str()) {
            continue;
        }
        // Skip if already on disk (may have been installed between bundled and here)
        if channel_wasm_on_disk(channels_dir, name) {
            continue;
        }

        // Look up in registry
        let Some(manifest) = catalogue.get(&format!("channels/{}", name)) else {
            continue;
        };

        if install_registry_channel(channels_dir, &repo_root, name, manifest).await {
            installed.push(name.clone());
        }
    }

    installed
}

/// Report whether a channel's WASM artefact already exists on disk under
/// either of its recognized filenames.
fn channel_wasm_on_disk(channels_dir: &std::path::Path, name: &str) -> bool {
    channels_dir.join(format!("{}.wasm", name)).exists()
        || channels_dir.join(format!("{}-channel.wasm", name)).exists()
}

/// Install one registry channel, reporting warnings and failures to the user.
///
/// Returns `true` when the install succeeded.
async fn install_registry_channel(
    channels_dir: &std::path::Path,
    repo_root: &std::path::Path,
    name: &str,
    manifest: &crate::registry::manifest::ExtensionManifest,
) -> bool {
    let installer = crate::registry::installer::RegistryInstaller::new(
        repo_root.to_path_buf(),
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
            true
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
            false
        }
    }
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
