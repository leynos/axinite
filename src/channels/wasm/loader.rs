//! WASM channel loader for loading channels from files or directories.
//!
//! Loads WASM channel modules from the filesystem (default: ~/.ironclaw/channels/).
//! Each channel consists of:
//! - `<name>.wasm` - The compiled WASM component
//! - `<name>.capabilities.json` - Channel capabilities and configuration

use std::path::Path;
use std::sync::Arc;

use tokio::fs;

use crate::channels::wasm::capabilities::ChannelCapabilities;
use crate::channels::wasm::error::WasmChannelError;
use crate::channels::wasm::runtime::WasmChannelRuntime;
use crate::channels::wasm::schema::ChannelCapabilitiesFile;
use crate::channels::wasm::wrapper::WasmChannel;
use crate::db::SettingsStore;
use crate::pairing::PairingStore;
use crate::secrets::SecretsStore;

/// Loads WASM channels from the filesystem.
pub struct WasmChannelLoader {
    runtime: Arc<WasmChannelRuntime>,
    pairing_store: Arc<PairingStore>,
    settings_store: Option<Arc<dyn SettingsStore>>,
    secrets_store: Option<Arc<dyn SecretsStore + Send + Sync>>,
}

/// Capabilities, config, and metadata resolved from a capabilities sidecar
/// file (or defaults when no usable sidecar exists).
struct ResolvedCapabilities {
    capabilities: ChannelCapabilities,
    config_json: String,
    description: Option<String>,
    cap_file: Option<ChannelCapabilitiesFile>,
}

/// Minimal default capabilities for a channel with no sidecar file.
fn default_capabilities(name: &str) -> ResolvedCapabilities {
    ResolvedCapabilities {
        capabilities: ChannelCapabilities::for_channel(name),
        config_json: "{}".to_string(),
        description: None,
        cap_file: None,
    }
}

/// Resolve channel capabilities from the optional sidecar path, falling back
/// to defaults when the path is absent or the file does not exist.
async fn resolve_capabilities(
    name: &str,
    capabilities_path: Option<&Path>,
) -> Result<ResolvedCapabilities, WasmChannelError> {
    let Some(cap_path) = capabilities_path else {
        return Ok(default_capabilities(name));
    };
    if !cap_path.exists() {
        tracing::warn!(
            path = %cap_path.display(),
            "Capabilities file not found, using defaults"
        );
        return Ok(default_capabilities(name));
    }
    parse_capabilities_file(name, cap_path).await
}

/// Parse and validate a capabilities sidecar file, checking WIT version
/// compatibility and logging the resulting capability grants.
async fn parse_capabilities_file(
    name: &str,
    cap_path: &Path,
) -> Result<ResolvedCapabilities, WasmChannelError> {
    let cap_bytes = fs::read(cap_path).await?;
    let cap_file = ChannelCapabilitiesFile::from_bytes(&cap_bytes)
        .map_err(|e| WasmChannelError::InvalidCapabilities(e.to_string()))?;
    cap_file.validate();

    // Debug: log raw capabilities
    tracing::debug!(
        channel = name,
        raw_capabilities = ?cap_file.capabilities,
        "Parsed capabilities file"
    );

    // Check WIT version compatibility
    crate::tools::wasm::loader::check_wit_version_compat(
        name,
        cap_file.wit_version.as_deref(),
        crate::tools::wasm::WIT_CHANNEL_VERSION,
    )
    .map_err(|e| WasmChannelError::IncompatibleWitVersion(e.to_string()))?;

    let caps = cap_file.to_capabilities();

    // Debug: log resulting capabilities
    tracing::info!(
        channel = name,
        http_allowed = caps.tool_capabilities.http.is_some(),
        http_allowlist_count = caps
            .tool_capabilities
            .http
            .as_ref()
            .map(|h| h.allowlist.len())
            .unwrap_or(0),
        "Channel capabilities loaded"
    );

    let config_json = cap_file.config_json();
    let description = cap_file.description.clone();

    Ok(ResolvedCapabilities {
        capabilities: caps,
        config_json,
        description,
        cap_file: Some(cap_file),
    })
}

/// Whether a channel name is empty or could escape the channel directory.
///
/// Rejects names containing path separators or `..` traversal segments.
fn is_invalid_channel_name(name: &str) -> bool {
    if name.is_empty() {
        return true;
    }
    crate::tools::wasm::loader::contains_path_separator(name) || name.contains("..")
}

impl WasmChannelLoader {
    /// Create a new loader with the given runtime and pairing store.
    pub fn new(
        runtime: Arc<WasmChannelRuntime>,
        pairing_store: Arc<PairingStore>,
        settings_store: Option<Arc<dyn SettingsStore>>,
    ) -> Self {
        Self {
            runtime,
            pairing_store,
            settings_store,
            secrets_store: None,
        }
    }

    /// Set the secrets store for host-based credential injection in WASM channels.
    pub fn with_secrets_store(mut self, store: Arc<dyn SecretsStore + Send + Sync>) -> Self {
        self.secrets_store = Some(store);
        self
    }

    /// Load a single WASM channel from a file pair.
    ///
    /// Expects:
    /// - `wasm_path`: Path to the `.wasm` file
    /// - `capabilities_path`: Path to the `.capabilities.json` file (optional)
    ///
    /// If no capabilities file is provided, the channel gets minimal capabilities.
    pub async fn load_from_files(
        &self,
        name: &str,
        wasm_path: &Path,
        capabilities_path: Option<&Path>,
    ) -> Result<LoadedChannel, WasmChannelError> {
        // Validate name
        if is_invalid_channel_name(name) {
            return Err(WasmChannelError::InvalidName(name.to_string()));
        }

        // Read WASM bytes
        if !wasm_path.exists() {
            return Err(WasmChannelError::WasmNotFound(wasm_path.to_path_buf()));
        }
        let wasm_bytes = fs::read(wasm_path).await?;

        // Read capabilities file
        let resolved = resolve_capabilities(name, capabilities_path).await?;

        // Prepare the module
        let prepared = self
            .runtime
            .prepare(name, &wasm_bytes, None, resolved.description)
            .await?;

        // Create the channel
        let mut channel = WasmChannel::new(
            self.runtime.clone(),
            prepared,
            resolved.capabilities,
            resolved.config_json,
            self.pairing_store.clone(),
            self.settings_store.clone(),
        );
        if let Some(ref secrets) = self.secrets_store {
            channel = channel.with_secrets_store(Arc::clone(secrets));
        }

        tracing::info!(
            name = name,
            wasm_path = %wasm_path.display(),
            "Loaded WASM channel from file"
        );

        Ok(LoadedChannel {
            channel,
            capabilities_file: resolved.cap_file,
        })
    }

    /// Load all WASM channels from a directory.
    ///
    /// Scans the directory for `*.wasm` files and loads each one, looking for
    /// a matching `*.capabilities.json` sidecar file.
    ///
    /// # Directory Layout
    ///
    /// ```text
    /// channels/
    /// ├── slack.wasm                  <- Channel WASM component
    /// ├── slack.capabilities.json     <- Capabilities (optional)
    /// ├── telegram.wasm
    /// └── telegram.capabilities.json
    /// ```
    pub async fn load_from_dir(&self, dir: &Path) -> Result<LoadResults, WasmChannelError> {
        match fs::metadata(dir).await.map(ambient_fs::Metadata::from_std) {
            Ok(meta) if meta.is_dir() => {}
            Ok(_) => {
                return Err(WasmChannelError::Io(std::io::Error::new(
                    std::io::ErrorKind::NotADirectory,
                    format!("{} is not a directory", dir.display()),
                )));
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                return Ok(LoadResults::default());
            }
            Err(e) => return Err(WasmChannelError::Io(e)),
        }

        let mut results = LoadResults::default();

        // Collect all .wasm entries first, then load in parallel
        let mut channel_entries = Vec::new();
        // Handle TOCTOU: if read_dir fails with NotFound, treat as empty
        let mut entries = match fs::read_dir(dir).await {
            Ok(entries) => entries,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                return Ok(LoadResults::default());
            }
            Err(e) => return Err(WasmChannelError::Io(e)),
        };

        while let Some(entry) = entries.next_entry().await? {
            let path = entry.path();

            if path.extension().and_then(|e| e.to_str()) != Some("wasm") {
                continue;
            }

            let name = match path.file_stem().and_then(|s| s.to_str()) {
                Some(n) => n.to_string(),
                None => {
                    results.errors.push((
                        path.clone(),
                        WasmChannelError::InvalidName("invalid filename".to_string()),
                    ));
                    continue;
                }
            };

            let cap_path = path.with_extension("capabilities.json");
            let has_cap = cap_path.exists();
            channel_entries.push((name, path, if has_cap { Some(cap_path) } else { None }));
        }

        // Load all channels in parallel (file I/O + WASM compilation)
        let load_futures = channel_entries
            .iter()
            .map(|(name, path, cap_path)| self.load_from_files(name, path, cap_path.as_deref()));

        let load_results = futures::future::join_all(load_futures).await;

        for ((name, path, _), result) in channel_entries.into_iter().zip(load_results) {
            match result {
                Ok(loaded) => {
                    results.loaded.push(loaded);
                }
                Err(e) => {
                    tracing::error!(
                        name = name,
                        path = %path.display(),
                        error = %e,
                        "Failed to load WASM channel"
                    );
                    results.errors.push((path, e));
                }
            }
        }

        if !results.loaded.is_empty() {
            tracing::info!(
                count = results.loaded.len(),
                channels = ?results.loaded.iter().map(|c| c.name()).collect::<Vec<_>>(),
                "Loaded WASM channels from directory"
            );
        }

        Ok(results)
    }
}
mod discovery;
mod results;

#[cfg(test)]
mod tests;

pub use discovery::{DiscoveredChannel, default_channels_dir, discover_channels};
pub use results::{LoadResults, LoadedChannel};
