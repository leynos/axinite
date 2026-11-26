//! Discovery of the capability files that plugin hooks are loaded from.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use crate::channels::wasm::discover_channels;
use crate::tools::wasm::{discover_dev_tools, discover_tools};

/// Whether a tool is active and was loaded from the bundled WASM directory
/// rather than the dev-tools override.
fn is_active_bundled_tool(
    active_tools: &HashSet<&str>,
    dev_loaded_tools: &HashSet<&str>,
    name: &str,
) -> bool {
    active_tools.contains(name) && !dev_loaded_tools.contains(name)
}

/// Whether a tool is active and was loaded via the dev-tools override.
fn is_active_dev_tool(
    active_tools: &HashSet<&str>,
    dev_loaded_tools: &HashSet<&str>,
    name: &str,
) -> bool {
    active_tools.contains(name) && dev_loaded_tools.contains(name)
}

/// Collect the (source, path) pairs of every capability file of an active
/// bundled tool, dev-override tool or channel, sorted by source.
pub(super) async fn collect_plugin_capability_files(
    wasm_tools_dir: &Path,
    wasm_channels_dir: &Path,
    active_tool_names: &[String],
    active_channel_names: &[String],
    dev_loaded_tool_names: &[String],
) -> Vec<(String, PathBuf)> {
    let mut collected = CapabilityFiles::default();
    let active_tools: HashSet<&str> = active_tool_names.iter().map(String::as_str).collect();
    let active_channels: HashSet<&str> = active_channel_names.iter().map(String::as_str).collect();
    let dev_loaded_tools: HashSet<&str> =
        dev_loaded_tool_names.iter().map(String::as_str).collect();

    collected
        .add_bundled_tools(wasm_tools_dir, &active_tools, &dev_loaded_tools)
        .await;
    collected
        .add_dev_tools(&active_tools, &dev_loaded_tools)
        .await;
    collected
        .add_channels(wasm_channels_dir, &active_channels)
        .await;

    collected.files.sort_by(|a, b| a.0.cmp(&b.0));
    collected.files
}

/// Capability files gathered for plugin hooks, de-duplicated by path.
#[derive(Default)]
struct CapabilityFiles {
    files: Vec<(String, PathBuf)>,
    seen: HashSet<String>,
}

impl CapabilityFiles {
    /// Record `path` under `source` unless that path is already present.
    fn insert_unique(&mut self, source: String, path: PathBuf) {
        let key = path.to_string_lossy().to_string();
        if self.seen.insert(key) {
            self.files.push((source, path));
        }
    }

    /// Add each discovered (name, capabilities path) pair whose name is
    /// active, recording it as `<source_prefix>:<name>`.
    fn add_discovered(
        &mut self,
        source_prefix: &str,
        discovered: impl IntoIterator<Item = (String, Option<PathBuf>)>,
        is_active: impl Fn(&str) -> bool,
    ) {
        for (name, path) in discovered {
            if let Some(path) = path
                && is_active(name.as_str())
            {
                self.insert_unique(format!("{source_prefix}:{name}"), path);
            }
        }
    }

    /// Add the capability files of active bundled WASM tools.
    async fn add_bundled_tools(
        &mut self,
        wasm_tools_dir: &Path,
        active_tools: &HashSet<&str>,
        dev_loaded_tools: &HashSet<&str>,
    ) {
        if !wasm_tools_dir.exists() {
            return;
        }
        match discover_tools(wasm_tools_dir).await {
            Ok(tools) => self.add_discovered(
                "plugin.tool",
                tools
                    .into_iter()
                    .map(|(name, tool)| (name, tool.capabilities_path)),
                |name| is_active_bundled_tool(active_tools, dev_loaded_tools, name),
            ),
            Err(err) => {
                tracing::warn!(
                    path = %wasm_tools_dir.display(),
                    error = %err,
                    "Failed to discover WASM tool capabilities for plugin hooks"
                );
            }
        }
    }

    /// Add the capability files of active dev-override tools.
    async fn add_dev_tools(
        &mut self,
        active_tools: &HashSet<&str>,
        dev_loaded_tools: &HashSet<&str>,
    ) {
        match discover_dev_tools().await {
            Ok(dev_tools) => self.add_discovered(
                "plugin.dev_tool",
                dev_tools
                    .into_iter()
                    .map(|(name, tool)| (name, tool.capabilities_path)),
                |name| is_active_dev_tool(active_tools, dev_loaded_tools, name),
            ),
            Err(err) => {
                tracing::debug!(
                    error = %err,
                    "No dev tool capabilities discovered for plugin hooks"
                );
            }
        }
    }

    /// Add the capability files of active WASM channels.
    async fn add_channels(&mut self, wasm_channels_dir: &Path, active_channels: &HashSet<&str>) {
        if !wasm_channels_dir.exists() {
            return;
        }
        match discover_channels(wasm_channels_dir).await {
            Ok(channels) => self.add_discovered(
                "plugin.channel",
                channels
                    .into_iter()
                    .map(|(name, channel)| (name, channel.capabilities_path)),
                |name| active_channels.contains(name),
            ),
            Err(err) => {
                tracing::warn!(
                    path = %wasm_channels_dir.display(),
                    error = %err,
                    "Failed to discover WASM channel capabilities for plugin hooks"
                );
            }
        }
    }
}

#[cfg(test)]
mod tests {
    //! Unit tests for capability file de-duplication.

    use super::*;

    #[test]
    fn insert_unique_keeps_the_first_source_for_a_path() {
        let mut collected = CapabilityFiles::default();
        collected.insert_unique("plugin.tool:a".to_string(), PathBuf::from("/x/caps.json"));
        collected.insert_unique(
            "plugin.channel:b".to_string(),
            PathBuf::from("/x/caps.json"),
        );
        collected.insert_unique("plugin.tool:c".to_string(), PathBuf::from("/y/caps.json"));

        let sources: Vec<&str> = collected.files.iter().map(|(s, _)| s.as_str()).collect();
        assert_eq!(sources, ["plugin.tool:a", "plugin.tool:c"]);
    }

    #[test]
    fn bundled_and_dev_tool_classification_is_exclusive() {
        let active: HashSet<&str> = ["a", "b"].into();
        let dev: HashSet<&str> = ["b"].into();

        assert!(is_active_bundled_tool(&active, &dev, "a"));
        assert!(!is_active_bundled_tool(&active, &dev, "b"));
        assert!(is_active_dev_tool(&active, &dev, "b"));
        assert!(!is_active_dev_tool(&active, &dev, "a"));
        assert!(!is_active_bundled_tool(&active, &dev, "inactive"));
    }
}
