//! Install routing: MCP config helpers, registry entries, sources, and per-kind installers.

use crate::extensions::{
    ExtensionError, ExtensionKind, ExtensionSource, InstallResult, RegistryEntry,
};
use crate::tools::mcp::config::McpServerConfig;

use super::ExtensionManager;
use super::{FallbackDecision, combine_install_errors, fallback_decision};

impl ExtensionManager {
    // ── MCP config helpers (DB with disk fallback) ─────────────────────

    pub(super) async fn load_mcp_servers(
        &self,
    ) -> Result<crate::tools::mcp::config::McpServersFile, crate::tools::mcp::config::ConfigError>
    {
        if let Some(ref store) = self.store {
            crate::tools::mcp::config::load_mcp_servers_from_db(store.as_ref(), &self.user_id).await
        } else {
            crate::tools::mcp::config::load_mcp_servers().await
        }
    }

    pub(super) async fn get_mcp_server(
        &self,
        name: &str,
    ) -> Result<McpServerConfig, crate::tools::mcp::config::ConfigError> {
        let servers = self.load_mcp_servers().await?;
        servers.get(name).cloned().ok_or_else(|| {
            crate::tools::mcp::config::ConfigError::ServerNotFound {
                name: name.to_string(),
            }
        })
    }

    pub(super) async fn add_mcp_server(
        &self,
        config: McpServerConfig,
    ) -> Result<(), crate::tools::mcp::config::ConfigError> {
        config.validate()?;
        if let Some(ref store) = self.store {
            crate::tools::mcp::config::add_mcp_server_db(store.as_ref(), &self.user_id, config)
                .await
        } else {
            crate::tools::mcp::config::add_mcp_server(config).await
        }
    }

    pub(super) async fn remove_mcp_server(
        &self,
        name: &str,
    ) -> Result<(), crate::tools::mcp::config::ConfigError> {
        if let Some(ref store) = self.store {
            crate::tools::mcp::config::remove_mcp_server_db(store.as_ref(), &self.user_id, name)
                .await
        } else {
            crate::tools::mcp::config::remove_mcp_server(name).await
        }
    }

    // ── Private helpers ──────────────────────────────────────────────────

    pub(super) async fn install_from_entry(
        &self,
        entry: &RegistryEntry,
    ) -> Result<InstallResult, ExtensionError> {
        let primary_result = self.try_install_from_source(entry, &entry.source).await;
        match fallback_decision(&primary_result, &entry.fallback_source) {
            FallbackDecision::Return => primary_result,
            FallbackDecision::TryFallback => {
                let primary_err = primary_result.unwrap_err();
                let fallback = entry.fallback_source.as_ref().unwrap();
                tracing::info!(
                    extension = %entry.name,
                    primary_error = %primary_err,
                    "Primary install failed, trying fallback source"
                );
                match self.try_install_from_source(entry, fallback).await {
                    Ok(result) => Ok(result),
                    Err(fallback_err) => {
                        tracing::error!(
                            extension = %entry.name,
                            fallback_error = %fallback_err,
                            "Fallback install also failed"
                        );
                        Err(combine_install_errors(primary_err, fallback_err))
                    }
                }
            }
        }
    }

    /// Attempt to install an extension using a specific source.
    pub(super) async fn try_install_from_source(
        &self,
        entry: &RegistryEntry,
        source: &ExtensionSource,
    ) -> Result<InstallResult, ExtensionError> {
        match entry.kind {
            ExtensionKind::McpServer => {
                let url = match source {
                    ExtensionSource::McpUrl { url } => url.clone(),
                    ExtensionSource::Discovered { url } => url.clone(),
                    _ => {
                        return Err(ExtensionError::InstallFailed(
                            "Registry entry for MCP server has no URL".to_string(),
                        ));
                    }
                };
                self.install_mcp_from_url(&entry.name, &url).await
            }
            ExtensionKind::WasmTool | ExtensionKind::WasmChannel => {
                self.install_wasm_from_source(entry, source, entry.kind)
                    .await
            }
            ExtensionKind::ChannelRelay => {
                // No download needed — just mark as installed.
                self.installed_relay_extensions
                    .write()
                    .await
                    .insert(entry.name.clone());
                Ok(InstallResult {
                    name: entry.name.clone(),
                    kind: ExtensionKind::ChannelRelay,
                    message: format!(
                        "'{}' installed. Click Activate to connect your workspace.",
                        entry.display_name
                    ),
                })
            }
        }
    }

    /// Install a WASM extension (tool or channel) from its source, routing
    /// to the download or buildable installer with the kind-appropriate
    /// target directory.
    async fn install_wasm_from_source(
        &self,
        entry: &RegistryEntry,
        source: &ExtensionSource,
        kind: ExtensionKind,
    ) -> Result<InstallResult, ExtensionError> {
        match source {
            ExtensionSource::WasmDownload {
                wasm_url,
                capabilities_url,
            } => {
                if kind == ExtensionKind::WasmTool {
                    self.install_wasm_tool_from_url_with_caps(
                        &entry.name,
                        wasm_url,
                        capabilities_url.as_deref(),
                    )
                    .await
                } else {
                    self.install_wasm_channel_from_url(
                        &entry.name,
                        wasm_url,
                        capabilities_url.as_deref(),
                    )
                    .await
                }
            }
            ExtensionSource::WasmBuildable {
                build_dir,
                crate_name,
                ..
            } => {
                let target_dir = if kind == ExtensionKind::WasmTool {
                    &self.wasm_tools_dir
                } else {
                    &self.wasm_channels_dir
                };
                self.install_wasm_from_buildable(
                    &entry.name,
                    build_dir.as_deref(),
                    crate_name.as_deref(),
                    target_dir,
                    kind,
                )
                .await
            }
            _ => {
                let label = if kind == ExtensionKind::WasmTool {
                    "WASM tool"
                } else {
                    "WASM channel"
                };
                Err(ExtensionError::InstallFailed(format!(
                    "{} entry has no download URL or build info",
                    label
                )))
            }
        }
    }

    pub(super) async fn install_mcp_from_url(
        &self,
        name: &str,
        url: &str,
    ) -> Result<InstallResult, ExtensionError> {
        // Check if already installed
        if self.get_mcp_server(name).await.is_ok() {
            return Err(ExtensionError::AlreadyInstalled(name.to_string()));
        }

        let config = McpServerConfig::new(name, url);
        config
            .validate()
            .map_err(|e| ExtensionError::InvalidUrl(e.to_string()))?;

        self.add_mcp_server(config)
            .await
            .map_err(|e| ExtensionError::Config(e.to_string()))?;

        tracing::info!("Installed MCP server '{}' at {}", name, url);

        Ok(InstallResult {
            name: name.to_string(),
            kind: ExtensionKind::McpServer,
            message: format!(
                "MCP server '{}' installed. Run auth next to authenticate.",
                name
            ),
        })
    }

    pub(super) async fn install_wasm_tool_from_url(
        &self,
        name: &str,
        url: &str,
    ) -> Result<InstallResult, ExtensionError> {
        self.install_wasm_tool_from_url_with_caps(name, url, None)
            .await
    }

    pub(super) async fn install_wasm_tool_from_url_with_caps(
        &self,
        name: &str,
        url: &str,
        capabilities_url: Option<&str>,
    ) -> Result<InstallResult, ExtensionError> {
        self.download_and_install_wasm(name, url, capabilities_url, &self.wasm_tools_dir)
            .await?;

        Ok(InstallResult {
            name: name.to_string(),
            kind: ExtensionKind::WasmTool,
            message: format!("WASM tool '{}' installed. Run activate to load it.", name),
        })
    }

    pub(super) async fn install_wasm_channel_from_url(
        &self,
        name: &str,
        url: &str,
        capabilities_url: Option<&str>,
    ) -> Result<InstallResult, ExtensionError> {
        self.download_and_install_wasm(name, url, capabilities_url, &self.wasm_channels_dir)
            .await?;

        Ok(InstallResult {
            name: name.to_string(),
            kind: ExtensionKind::WasmChannel,
            message: format!(
                "WASM channel '{}' installed. Run activate to start it.",
                name,
            ),
        })
    }
}
