//! Core operations: search, install, auth, activate dispatch, and kind/name validation.

use crate::extensions::{
    ActivateResult, AuthResult, ExtensionError, ExtensionKind, InstallResult, ResultSource,
    SearchResult,
};

use super::ExtensionManager;
use super::{infer_kind_from_url, sanitize_url_for_logging};

impl ExtensionManager {
    /// Search for extensions. If `discover` is true, also searches online.
    pub async fn search(
        &self,
        query: &str,
        discover: bool,
    ) -> Result<Vec<SearchResult>, ExtensionError> {
        let mut results = self.registry.search(query).await;

        if discover && results.is_empty() {
            tracing::info!("No built-in results for '{}', searching online...", query);
            let discovered = self.discovery.discover(query).await;

            if !discovered.is_empty() {
                // Cache for future lookups
                self.registry.cache_discovered(discovered.clone()).await;

                // Add to results
                for entry in discovered {
                    results.push(SearchResult {
                        entry,
                        source: ResultSource::Discovered,
                        validated: true,
                    });
                }
            }
        }

        Ok(results)
    }

    /// Install an extension by name (from registry) or by explicit URL.
    pub async fn install(
        &self,
        name: &str,
        url: Option<&str>,
        kind_hint: Option<ExtensionKind>,
    ) -> Result<InstallResult, ExtensionError> {
        let sanitized_url = url.map(sanitize_url_for_logging);
        tracing::info!(extension = %name, url = ?sanitized_url, kind = ?kind_hint, "Installing extension");
        Self::validate_extension_name(name)?;

        // If we have a registry entry, use it (prefer kind_hint to resolve collisions)
        if let Some(entry) = self.registry.get_with_kind(name, kind_hint).await {
            return self.install_from_entry(&entry).await.map_err(|e| {
                tracing::error!(extension = %name, error = %e, "Extension install failed");
                e
            });
        }

        // If a URL was provided, determine kind and install
        if let Some(url) = url {
            let kind = kind_hint.unwrap_or_else(|| infer_kind_from_url(url));
            return match kind {
                ExtensionKind::McpServer => self.install_mcp_from_url(name, url).await,
                ExtensionKind::WasmTool => self.install_wasm_tool_from_url(name, url).await,
                ExtensionKind::WasmChannel => {
                    self.install_wasm_channel_from_url(name, url, None).await
                }
                ExtensionKind::ChannelRelay => {
                    // ChannelRelay extensions are installed from registry, not by URL
                    Err(ExtensionError::InstallFailed(
                        "Channel relay extensions cannot be installed by URL".to_string(),
                    ))
                }
            }
            .map_err(|e| {
                let sanitized = sanitize_url_for_logging(url);
                tracing::error!(extension = %name, url = %sanitized, error = %e, "Extension install from URL failed");
                e
            });
        }

        let err = ExtensionError::NotFound(format!(
            "'{}' not found in registry. Try searching with discover:true or provide a URL.",
            name
        ));
        tracing::warn!(extension = %name, "Extension not found in registry");
        Err(err)
    }

    /// Authenticate an installed extension.
    pub async fn auth(
        &self,
        name: &str,
        token: Option<&str>,
    ) -> Result<AuthResult, ExtensionError> {
        // Clean up expired pending auths
        self.cleanup_expired_auths().await;

        // Determine what kind of extension this is
        let kind = self.determine_installed_kind(name).await?;

        match kind {
            ExtensionKind::McpServer => self.auth_mcp(name, token).await,
            ExtensionKind::WasmTool => self.auth_wasm_tool(name, token).await,
            ExtensionKind::WasmChannel => self.auth_wasm_channel(name, token).await,
            ExtensionKind::ChannelRelay => self.auth_channel_relay(name, token).await,
        }
    }

    /// Activate an installed (and optionally authenticated) extension.
    pub async fn activate(&self, name: &str) -> Result<ActivateResult, ExtensionError> {
        Self::validate_extension_name(name)?;
        let kind = self.determine_installed_kind(name).await?;

        match kind {
            ExtensionKind::McpServer => self.mcp_activation.activate_mcp(name).await,
            ExtensionKind::WasmTool => self.wasm_tool_activation.activate_wasm_tool(name).await,
            ExtensionKind::WasmChannel => {
                self.wasm_channel_activation
                    .activate_wasm_channel(name)
                    .await
            }
            ExtensionKind::ChannelRelay => {
                self.wasm_channel_activation
                    .activate_channel_relay(name)
                    .await
            }
        }
    }

    /// Whether a listing filtered by `kind_filter` should include extensions
    /// of `kind`.
    pub(super) fn kind_selected(kind_filter: Option<ExtensionKind>, kind: ExtensionKind) -> bool {
        kind_filter.is_none() || kind_filter == Some(kind)
    }

    /// Determine what kind of installed extension this is.
    ///
    /// This is a read-only check — it never modifies `installed_relay_extensions`.
    /// To mark a relay extension as installed, use `activate_stored_relay()` or
    /// the explicit install flow.
    pub(super) async fn determine_installed_kind(
        &self,
        name: &str,
    ) -> Result<ExtensionKind, ExtensionError> {
        // Check MCP servers first
        if self.get_mcp_server(name).await.is_ok() {
            return Ok(ExtensionKind::McpServer);
        }

        // Check WASM tools
        let wasm_path = self.wasm_tools_dir.join(format!("{}.wasm", name));
        if wasm_path.exists() {
            return Ok(ExtensionKind::WasmTool);
        }

        // Check WASM channels
        let channel_path = self.wasm_channels_dir.join(format!("{}.wasm", name));
        if channel_path.exists() {
            return Ok(ExtensionKind::WasmChannel);
        }

        // Check channel-relay extensions (installed in memory or has stored token)
        if self.installed_relay_extensions.read().await.contains(name) {
            return Ok(ExtensionKind::ChannelRelay);
        }
        // Also check if there's a stored stream token (persisted across restarts)
        if self
            .secrets
            .exists(&self.user_id, &format!("relay:{}:stream_token", name))
            .await
            .unwrap_or(false)
        {
            return Ok(ExtensionKind::ChannelRelay);
        }

        Err(ExtensionError::NotInstalled(format!(
            "'{}' is not installed as an MCP server, WASM tool, WASM channel, or channel relay",
            name
        )))
    }

    /// Whether the name contains a path separator, traversal sequence, or NUL.
    pub(super) fn has_unsafe_name_characters(name: &str) -> bool {
        let has_separator = name.contains('/') || name.contains('\\');
        let has_traversal_or_nul = name.contains("..") || name.contains('\0');
        has_separator || has_traversal_or_nul
    }

    /// Reject names containing path separators or traversal sequences.
    pub(super) fn validate_extension_name(name: &str) -> Result<(), ExtensionError> {
        if Self::has_unsafe_name_characters(name) {
            return Err(ExtensionError::InstallFailed(format!(
                "Invalid extension name '{}': contains path separator or traversal characters",
                name
            )));
        }
        Ok(())
    }

    pub(super) async fn cleanup_expired_auths(&self) {
        let mut pending = self.pending_auth.write().await;
        pending.retain(|_, auth| {
            let expired = auth.created_at.elapsed() >= std::time::Duration::from_secs(300);
            if expired {
                // Abort the background listener task to free port 9876
                if let Some(ref handle) = auth.task_handle {
                    handle.abort();
                }
            }
            !expired
        });
    }
}
