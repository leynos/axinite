//! The [`WasmToolLoader`]: loads WASM tools from file pairs, directories,
//! or database storage into the tool registry.

use std::path::Path;
use std::sync::Arc;

use tokio::fs;

use crate::secrets::SecretsStore;
use crate::tools::registry::{ToolRegistry, WasmFromStorageRegistration, WasmToolRegistration};
use crate::tools::wasm::capabilities_schema::CapabilitiesFile;
use crate::tools::wasm::{Capabilities, OAuthRefreshConfig, WasmToolRuntime, WasmToolStore};

use super::wit_compat::check_wit_version_compat;
use super::{LoadResults, WasmLoadError, contains_path_separator};

/// Loads WASM tools from files or storage into the registry.
pub struct WasmToolLoader {
    runtime: Arc<WasmToolRuntime>,
    registry: Arc<ToolRegistry>,
    secrets_store: Option<Arc<dyn SecretsStore + Send + Sync>>,
}

impl WasmToolLoader {
    /// Create a new loader with the given runtime and registry.
    pub fn new(runtime: Arc<WasmToolRuntime>, registry: Arc<ToolRegistry>) -> Self {
        Self {
            runtime,
            registry,
            secrets_store: None,
        }
    }

    /// Set the secrets store for credential injection in WASM tools.
    pub fn with_secrets_store(mut self, store: Arc<dyn SecretsStore + Send + Sync>) -> Self {
        self.secrets_store = Some(store);
        self
    }

    /// Load a single WASM tool from a file pair.
    ///
    /// Expects:
    /// - `wasm_path`: Path to the `.wasm` file
    /// - `capabilities_path`: Path to the `.capabilities.json` file (optional)
    ///
    /// If no capabilities file is provided, the tool gets no capabilities (default deny).
    pub async fn load_from_files(
        &self,
        name: &str,
        wasm_path: &Path,
        capabilities_path: Option<&Path>,
    ) -> Result<(), WasmLoadError> {
        if name.is_empty() || contains_path_separator(name) {
            return Err(WasmLoadError::InvalidName(name.to_string()));
        }

        // Read WASM bytes
        if !wasm_path.exists() {
            return Err(WasmLoadError::WasmNotFound(wasm_path.to_path_buf()));
        }
        let wasm_bytes = fs::read(wasm_path).await?;

        // Read capabilities (optional) and extract OAuth refresh config
        let (capabilities, oauth_refresh) = if let Some(cap_path) = capabilities_path {
            if cap_path.exists() {
                let cap_bytes = fs::read(cap_path).await?;
                let cap_file = CapabilitiesFile::from_bytes(&cap_bytes)
                    .map_err(|e| WasmLoadError::InvalidCapabilities(e.to_string()))?;
                cap_file.validate(name);

                // Check WIT version compatibility
                check_wit_version_compat(
                    name,
                    cap_file.wit_version.as_deref(),
                    crate::tools::wasm::WIT_TOOL_VERSION,
                )?;

                let caps = cap_file.to_capabilities();
                let oauth = resolve_oauth_refresh_config(&cap_file);
                (caps, oauth)
            } else {
                tracing::warn!(
                    path = %cap_path.display(),
                    "Capabilities file not found, using default (no permissions)"
                );
                (Capabilities::default(), None)
            }
        } else {
            (Capabilities::default(), None)
        };

        // Register the tool
        self.registry
            .register_wasm(WasmToolRegistration {
                name,
                wasm_bytes: &wasm_bytes,
                runtime: &self.runtime,
                capabilities,
                limits: None,
                description: None,
                schema: None,
                secrets_store: self.secrets_store.clone(),
                oauth_refresh,
            })
            .await?;

        tracing::info!(
            name = name,
            wasm_path = %wasm_path.display(),
            "Loaded WASM tool from file"
        );

        Ok(())
    }

    /// Load all WASM tools from a directory.
    ///
    /// Scans the directory for `*.wasm` files and loads each one, looking for
    /// a matching `*.capabilities.json` sidecar file.
    ///
    /// # Directory Layout
    ///
    /// ```text
    /// tools/
    /// ├── slack.wasm                  <- Tool WASM component
    /// ├── slack.capabilities.json     <- Capabilities (optional)
    /// ├── github.wasm
    /// └── github.capabilities.json
    /// ```
    ///
    /// Tools without a capabilities file get no permissions (default deny).
    pub async fn load_from_dir(&self, dir: &Path) -> Result<LoadResults, WasmLoadError> {
        match fs::metadata(dir).await.map(ambient_fs::Metadata::from_std) {
            Ok(meta) if meta.is_dir() => {}
            Ok(_) => {
                return Err(WasmLoadError::Io(std::io::Error::new(
                    std::io::ErrorKind::NotADirectory,
                    format!("{} is not a directory", dir.display()),
                )));
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                return Ok(LoadResults::default());
            }
            Err(e) => return Err(WasmLoadError::Io(e)),
        }

        // Handle TOCTOU: if read_dir fails with NotFound, treat as empty
        let mut entries = match fs::read_dir(dir).await {
            Ok(entries) => entries,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                return Ok(LoadResults::default());
            }
            Err(e) => return Err(WasmLoadError::Io(e)),
        };

        let mut results = LoadResults::default();
        let mut tool_entries = Vec::new();

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
                        WasmLoadError::InvalidName("invalid filename".to_string()),
                    ));
                    continue;
                }
            };

            let cap_path = path.with_extension("capabilities.json");
            let has_cap = cap_path.exists();
            tool_entries.push((name, path, if has_cap { Some(cap_path) } else { None }));
        }

        // Load all tools in parallel (file I/O + WASM compilation + registration)
        let load_futures = tool_entries
            .iter()
            .map(|(name, path, cap_path)| self.load_from_files(name, path, cap_path.as_deref()));

        let load_results = futures::future::join_all(load_futures).await;

        for ((name, path, _), result) in tool_entries.into_iter().zip(load_results) {
            match result {
                Ok(()) => {
                    results.loaded.push(name);
                }
                Err(e) => {
                    tracing::error!(
                        name = name,
                        path = %path.display(),
                        error = %e,
                        "Failed to load WASM tool"
                    );
                    results.errors.push((path, e));
                }
            }
        }

        if !results.loaded.is_empty() {
            tracing::info!(
                count = results.loaded.len(),
                tools = ?results.loaded,
                "Loaded WASM tools from directory"
            );
        }

        Ok(results)
    }

    /// Load a WASM tool from database storage.
    ///
    /// This is a convenience wrapper around [`ToolRegistry::register_wasm_from_storage`].
    pub async fn load_from_storage(
        &self,
        store: &dyn WasmToolStore,
        user_id: &str,
        tool_name: &str,
    ) -> Result<(), WasmLoadError> {
        self.registry
            .register_wasm_from_storage(WasmFromStorageRegistration {
                store,
                runtime: &self.runtime,
                user_id,
                name: tool_name,
            })
            .await?;

        tracing::info!(
            user_id = user_id,
            name = tool_name,
            "Loaded WASM tool from storage"
        );

        Ok(())
    }

    /// Load all active WASM tools for a user from storage.
    pub async fn load_all_from_storage(
        &self,
        store: &dyn WasmToolStore,
        user_id: &str,
    ) -> Result<LoadResults, WasmLoadError> {
        let tools = store.list(user_id).await?;
        let mut results = LoadResults::default();

        for tool in tools {
            // Skip non-active tools
            if tool.status != crate::tools::wasm::ToolStatus::Active {
                continue;
            }

            match self.load_from_storage(store, user_id, &tool.name).await {
                Ok(()) => {
                    results.loaded.push(tool.name);
                }
                Err(e) => {
                    tracing::error!(
                        name = tool.name,
                        user_id = user_id,
                        error = %e,
                        "Failed to load WASM tool from storage"
                    );
                    results
                        .errors
                        .push((std::path::PathBuf::from(&tool.name), e));
                }
            }
        }

        Ok(results)
    }
}

/// Extract OAuth refresh configuration from a parsed capabilities file.
///
/// Returns `None` if there's no `auth.oauth` section or if the client_id
/// can't be resolved from any source (inline, env var, or built-in defaults).
///
/// Fallback chain for client_id:
///   `oauth.client_id` > env var (`oauth.client_id_env`) > `builtin_credentials()`
pub(super) fn resolve_oauth_refresh_config(
    cap_file: &CapabilitiesFile,
) -> Option<OAuthRefreshConfig> {
    let auth = cap_file.auth.as_ref()?;
    let oauth = auth.oauth.as_ref()?;

    let builtin = crate::cli::oauth_defaults::builtin_credentials(&auth.secret_name);

    let client_id = oauth
        .client_id
        .clone()
        .or_else(|| {
            oauth
                .client_id_env
                .as_ref()
                .and_then(|env| std::env::var(env).ok())
        })
        .or_else(|| builtin.as_ref().map(|c| c.client_id.to_string()))?;

    let client_secret = oauth
        .client_secret
        .clone()
        .or_else(|| {
            oauth
                .client_secret_env
                .as_ref()
                .and_then(|env| std::env::var(env).ok())
        })
        .or_else(|| builtin.as_ref().map(|c| c.client_secret.to_string()));

    Some(OAuthRefreshConfig {
        token_url: oauth.token_url.clone(),
        client_id,
        client_secret,
        secret_name: auth.secret_name.clone(),
        provider: auth.provider.clone(),
    })
}
