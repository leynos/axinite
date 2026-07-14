//! Discovery of installed WASM tool files in a directory, without loading
//! them.

use std::collections::HashMap;
use std::path::Path;

use tokio::fs;

use super::DiscoveredTool;

/// Discover WASM tool files in a directory without loading them.
///
/// Returns a map of tool name -> (wasm_path, capabilities_path).
pub async fn discover_tools(dir: &Path) -> Result<HashMap<String, DiscoveredTool>, std::io::Error> {
    let mut tools = HashMap::new();

    if !dir.is_dir() {
        return Ok(tools);
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

        tools.insert(
            name,
            DiscoveredTool {
                wasm_path: path,
                capabilities_path: if cap_path.exists() {
                    Some(cap_path)
                } else {
                    None
                },
            },
        );
    }

    Ok(tools)
}
