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

/// Where plugin capability files are discovered and which plugins are active.
pub(super) struct PluginCapabilitySources<'a> {
    pub(super) wasm_tools_dir: &'a Path,
    pub(super) wasm_channels_dir: &'a Path,
    pub(super) active_tool_names: &'a [String],
    pub(super) active_channel_names: &'a [String],
    pub(super) dev_loaded_tool_names: &'a [String],
}

impl PluginCapabilitySources<'_> {
    /// Collect the (source, path) pairs of every capability file of an active
    /// bundled tool, dev-override tool or channel, sorted by source.
    pub(super) async fn collect(&self) -> Vec<(String, PathBuf)> {
        let mut collected = CapabilityFiles::default();
        let active_tools = name_set(self.active_tool_names);
        let active_channels = name_set(self.active_channel_names);
        let dev_loaded_tools = name_set(self.dev_loaded_tool_names);

        collected
            .add_outcome(
                &TOOLS,
                self.wasm_tools_dir,
                discover_tool_paths(self.wasm_tools_dir).await,
                |name| is_active_bundled_tool(&active_tools, &dev_loaded_tools, name),
            )
            .add_dev_tools(&active_tools, &dev_loaded_tools)
            .await
            .add_outcome(
                &CHANNELS,
                self.wasm_channels_dir,
                discover_channel_paths(self.wasm_channels_dir).await,
                |name| active_channels.contains(name),
            );

        collected.files.sort_by(|a, b| a.0.cmp(&b.0));
        collected.files
    }
}

/// The names as a set of string slices.
fn name_set(names: &[String]) -> HashSet<&str> {
    names.iter().map(String::as_str).collect()
}

/// What discovery found: (name, capabilities path) pairs, or why it failed.
type Discovered = Result<Vec<(String, Option<PathBuf>)>, String>;

/// How one kind of plugin is labelled in sources and failure logs.
struct DiscoveryKind {
    /// Human-readable label for the failure log.
    label: &'static str,
    /// Prefix of the recorded source, followed by `:<name>`.
    prefix: &'static str,
}

const TOOLS: DiscoveryKind = DiscoveryKind {
    label: "WASM tool",
    prefix: "plugin.tool",
};

const CHANNELS: DiscoveryKind = DiscoveryKind {
    label: "WASM channel",
    prefix: "plugin.channel",
};

/// Discover bundled WASM tools; `None` when the directory does not exist.
async fn discover_tool_paths(dir: &Path) -> Option<Discovered> {
    if !dir.exists() {
        return None;
    }
    Some(
        discover_tools(dir)
            .await
            .map(|tools| {
                tools
                    .into_iter()
                    .map(|(name, tool)| (name, tool.capabilities_path))
                    .collect()
            })
            .map_err(|err| err.to_string()),
    )
}

/// Discover WASM channels; `None` when the directory does not exist.
async fn discover_channel_paths(dir: &Path) -> Option<Discovered> {
    if !dir.exists() {
        return None;
    }
    Some(
        discover_channels(dir)
            .await
            .map(|channels| {
                channels
                    .into_iter()
                    .map(|(name, channel)| (name, channel.capabilities_path))
                    .collect()
            })
            .map_err(|err| err.to_string()),
    )
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

    /// Add the active entries of one discovery outcome, or log why discovery
    /// failed. `None` (no such directory) adds nothing.
    fn add_outcome(
        &mut self,
        kind: &DiscoveryKind,
        dir: &Path,
        outcome: Option<Discovered>,
        is_active: impl Fn(&str) -> bool,
    ) -> &mut Self {
        match outcome {
            Some(Ok(discovered)) => self.add_discovered(kind.prefix, discovered, is_active),
            Some(Err(error)) => {
                tracing::warn!(
                    path = %dir.display(),
                    error = %error,
                    "Failed to discover {} capabilities for plugin hooks",
                    kind.label
                );
            }
            None => {}
        }
        self
    }

    /// Add the capability files of active dev-override tools.
    async fn add_dev_tools(
        &mut self,
        active_tools: &HashSet<&str>,
        dev_loaded_tools: &HashSet<&str>,
    ) -> &mut Self {
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
        self
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

    #[test]
    fn add_outcome_skips_inactive_names_and_missing_paths() {
        let mut collected = CapabilityFiles::default();
        let found = vec![
            ("on".to_string(), Some(PathBuf::from("/p/on.json"))),
            ("off".to_string(), Some(PathBuf::from("/p/off.json"))),
            ("bare".to_string(), None),
        ];
        collected.add_outcome(&TOOLS, Path::new("/p"), Some(Ok(found)), |name| {
            name != "off"
        });
        collected.add_outcome(
            &TOOLS,
            Path::new("/p"),
            Some(Err("boom".to_string())),
            |_| true,
        );
        collected.add_outcome(&CHANNELS, Path::new("/p"), None, |_| true);

        let sources: Vec<&str> = collected.files.iter().map(|(s, _)| s.as_str()).collect();
        assert_eq!(sources, ["plugin.tool:on"]);
    }
}
