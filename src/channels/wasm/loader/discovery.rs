//! Filesystem discovery of WASM channel files.
//!
//! Scans a directory for `*.wasm` files and matching `*.capabilities.json`
//! sidecars without loading them, and resolves the default channels directory.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use tokio::fs;

use crate::bootstrap::ironclaw_base_dir;

/// Discover WASM channel files in a directory without loading them.
///
/// Returns a map of channel name -> (wasm_path, capabilities_path).
#[allow(dead_code)]
pub async fn discover_channels(
    dir: &Path,
) -> Result<HashMap<String, DiscoveredChannel>, std::io::Error> {
    let mut channels = HashMap::new();

    if !dir.is_dir() {
        return Ok(channels);
    }

    let mut entries = fs::read_dir(dir).await?;

    while let Some(entry) = entries.next_entry().await? {
        let path = entry.path();

        if path.extension().and_then(|e| e.to_str()) != Some("wasm") {
            continue;
        }

        let name = match path.file_stem().and_then(|s| s.to_str()) {
            Some(n) => n.to_string(),
            None => continue,
        };

        let cap_path = path.with_extension("capabilities.json");

        channels.insert(
            name,
            DiscoveredChannel {
                wasm_path: path,
                capabilities_path: if cap_path.exists() {
                    Some(cap_path)
                } else {
                    None
                },
            },
        );
    }

    Ok(channels)
}

/// A discovered WASM channel (not yet loaded).
#[derive(Debug)]
pub struct DiscoveredChannel {
    /// Path to the WASM file.
    pub wasm_path: PathBuf,

    /// Path to the capabilities file (if present).
    pub capabilities_path: Option<PathBuf>,
}

/// Get the default channels directory path.
///
/// Returns ~/.ironclaw/channels/
#[allow(dead_code)]
pub fn default_channels_dir() -> PathBuf {
    ironclaw_base_dir().join("channels")
}
