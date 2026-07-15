//! Extension listing and detailed extension info reporting.

use crate::extensions::{ExtensionError, ExtensionKind, InstalledExtension, ToolAuthState};
use crate::tools::mcp::auth::is_authenticated;
use crate::tools::wasm::discover_tools;

use super::ExtensionManager;

impl ExtensionManager {
    /// List extensions with their status.
    ///
    /// When `include_available` is `true`, registry entries that are not yet
    /// installed are appended with `installed: false`.
    pub async fn list(
        &self,
        kind_filter: Option<ExtensionKind>,
        include_available: bool,
    ) -> Result<Vec<InstalledExtension>, ExtensionError> {
        let mut extensions = Vec::new();

        if Self::kind_selected(kind_filter, ExtensionKind::McpServer) {
            self.list_mcp_servers_into(&mut extensions).await;
        }
        if Self::kind_selected(kind_filter, ExtensionKind::WasmTool) && self.wasm_tools_dir.exists()
        {
            self.list_wasm_tools_into(&mut extensions).await;
        }
        if Self::kind_selected(kind_filter, ExtensionKind::WasmChannel)
            && self.wasm_channels_dir.exists()
        {
            self.list_wasm_channels_into(&mut extensions).await;
        }
        if Self::kind_selected(kind_filter, ExtensionKind::ChannelRelay) {
            self.list_relay_extensions_into(&mut extensions).await;
        }

        // Append available-but-not-installed registry entries
        if include_available {
            self.append_available_entries(kind_filter, &mut extensions)
                .await;
        }

        Ok(extensions)
    }

    /// Append installed MCP servers with auth/active state.
    async fn list_mcp_servers_into(&self, extensions: &mut Vec<InstalledExtension>) {
        let servers = match self.load_mcp_servers().await {
            Ok(servers) => servers,
            Err(e) => {
                tracing::debug!("Failed to load MCP servers for listing: {}", e);
                return;
            }
        };
        for server in &servers.servers {
            let authenticated = is_authenticated(server, &self.secrets, &self.user_id).await;
            let clients = self.mcp_clients.read().await;
            let active = clients.contains_key(&server.name);

            // Get tool names if active
            let tools = if active {
                self.tool_registry
                    .list()
                    .await
                    .into_iter()
                    .filter(|t| t.starts_with(&format!("{}_", server.name)))
                    .collect()
            } else {
                Vec::new()
            };

            let display_name = self
                .registry
                .get_with_kind(&server.name, Some(ExtensionKind::McpServer))
                .await
                .map(|e| e.display_name);
            extensions.push(InstalledExtension {
                name: server.name.clone(),
                kind: ExtensionKind::McpServer,
                display_name,
                description: server.description.clone(),
                url: Some(server.url.clone()),
                authenticated,
                active,
                tools,
                needs_setup: false,
                has_auth: false,
                installed: true,
                activation_error: None,
                version: None,
            });
        }
    }

    /// Append discovered WASM tools with auth/active state and versions.
    async fn list_wasm_tools_into(&self, extensions: &mut Vec<InstalledExtension>) {
        let tools = match discover_tools(&self.wasm_tools_dir).await {
            Ok(tools) => tools,
            Err(e) => {
                tracing::debug!("Failed to discover WASM tools for listing: {}", e);
                return;
            }
        };
        for (name, discovered) in tools {
            let active = self.tool_registry.has(&name).await;

            let registry_entry = self
                .registry
                .get_with_kind(&name, Some(ExtensionKind::WasmTool))
                .await;
            let display_name = registry_entry.as_ref().map(|e| e.display_name.clone());
            let auth_state = self.check_tool_auth_status(&name).await;
            let version = Self::capability_version(
                discovered.capabilities_path.as_deref(),
                |bytes| crate::tools::wasm::CapabilitiesFile::from_bytes(bytes).ok(),
                |cap| cap.version,
                registry_entry.and_then(|e| e.version.clone()),
            )
            .await;
            extensions.push(InstalledExtension {
                name: name.clone(),
                kind: ExtensionKind::WasmTool,
                display_name,
                description: None,
                url: None,
                authenticated: auth_state == ToolAuthState::Ready,
                active,
                tools: if active { vec![name] } else { Vec::new() },
                needs_setup: auth_state == ToolAuthState::NeedsSetup,
                has_auth: auth_state != ToolAuthState::NoAuth,
                installed: true,
                activation_error: None,
                version,
            });
        }
    }

    /// Append discovered WASM channels with auth/active state and versions.
    async fn list_wasm_channels_into(&self, extensions: &mut Vec<InstalledExtension>) {
        let channels = match crate::channels::wasm::discover_channels(&self.wasm_channels_dir).await
        {
            Ok(channels) => channels,
            Err(e) => {
                tracing::debug!("Failed to discover WASM channels for listing: {}", e);
                return;
            }
        };
        let active_names = self.active_channel_names.read().await;
        let errors = self.activation_errors.read().await;
        for (name, discovered) in channels {
            let active = active_names.contains(&name);
            let auth_state = self.check_channel_auth_status(&name).await;
            let activation_error = errors.get(&name).cloned();
            let registry_entry = self
                .registry
                .get_with_kind(&name, Some(ExtensionKind::WasmChannel))
                .await;
            let display_name = registry_entry.as_ref().map(|e| e.display_name.clone());
            let version = Self::capability_version(
                discovered.capabilities_path.as_deref(),
                |bytes| crate::channels::wasm::ChannelCapabilitiesFile::from_bytes(bytes).ok(),
                |cap| cap.version,
                registry_entry.and_then(|e| e.version.clone()),
            )
            .await;
            extensions.push(InstalledExtension {
                name,
                kind: ExtensionKind::WasmChannel,
                display_name,
                description: None,
                url: None,
                authenticated: auth_state == ToolAuthState::Ready,
                active,
                tools: Vec::new(),
                needs_setup: auth_state == ToolAuthState::NeedsSetup,
                has_auth: false,
                installed: true,
                activation_error,
                version,
            });
        }
    }

    /// Append installed channel-relay extensions.
    async fn list_relay_extensions_into(&self, extensions: &mut Vec<InstalledExtension>) {
        let installed = self.installed_relay_extensions.read().await;
        let active_names = self.active_channel_names.read().await;
        for name in installed.iter() {
            let active = active_names.contains(name);
            let has_token = self
                .secrets
                .exists(&self.user_id, &format!("relay:{}:stream_token", name))
                .await
                .unwrap_or(false);
            let registry_entry = self
                .registry
                .get_with_kind(name, Some(ExtensionKind::ChannelRelay))
                .await;
            let display_name = registry_entry.as_ref().map(|e| e.display_name.clone());
            let description = registry_entry.as_ref().map(|e| e.description.clone());
            extensions.push(InstalledExtension {
                name: name.clone(),
                kind: ExtensionKind::ChannelRelay,
                display_name,
                description,
                url: None,
                authenticated: has_token,
                active,
                tools: Vec::new(),
                needs_setup: false,
                has_auth: true,
                installed: true,
                activation_error: None,
                version: None,
            });
        }
    }

    /// Append registry entries that are not yet installed.
    async fn append_available_entries(
        &self,
        kind_filter: Option<ExtensionKind>,
        extensions: &mut Vec<InstalledExtension>,
    ) {
        let installed_names: std::collections::HashSet<(String, ExtensionKind)> = extensions
            .iter()
            .map(|e| (e.name.clone(), e.kind))
            .collect();

        for entry in self.registry.all_entries().await {
            if let Some(filter) = kind_filter
                && entry.kind != filter
            {
                continue;
            }
            if installed_names.contains(&(entry.name.clone(), entry.kind)) {
                continue;
            }
            extensions.push(InstalledExtension {
                name: entry.name,
                kind: entry.kind,
                display_name: Some(entry.display_name),
                description: Some(entry.description),
                url: None,
                authenticated: false,
                active: false,
                tools: Vec::new(),
                needs_setup: false,
                has_auth: false,
                installed: false,
                activation_error: None,
                version: entry.version,
            });
        }
    }

    /// Read and parse a capabilities file, returning `None` when the file is
    /// absent, unreadable, or fails to parse.
    pub(super) async fn load_capabilities<T>(
        cap_path: &std::path::Path,
        parse: impl FnOnce(&[u8]) -> Option<T>,
    ) -> Option<T> {
        if !cap_path.exists() {
            return None;
        }
        let bytes = tokio::fs::read(cap_path).await.ok()?;
        parse(&bytes)
    }

    /// Resolve the display version for a discovered WASM extension: prefer the
    /// version declared in its capabilities file, then fall back to the
    /// registry entry's version.
    ///
    /// Shared by the tool and channel listers, whose capabilities files parse
    /// to different types but expose the version the same way.
    async fn capability_version<T>(
        cap_path: Option<&std::path::Path>,
        parse: impl FnOnce(&[u8]) -> Option<T>,
        to_version: impl FnOnce(T) -> Option<String>,
        registry_version: Option<String>,
    ) -> Option<String> {
        let from_cap = match cap_path {
            Some(path) => Self::load_capabilities(path, parse)
                .await
                .and_then(to_version),
            None => None,
        };
        from_cap.or(registry_version)
    }

    /// Get detailed info about an installed extension (version, wit_version, host compatibility).
    pub async fn extension_info(&self, name: &str) -> Result<serde_json::Value, ExtensionError> {
        Self::validate_extension_name(name)?;
        let kind = self.determine_installed_kind(name).await?;

        let info = match kind {
            ExtensionKind::WasmTool => self.wasm_tool_info(name).await,
            ExtensionKind::WasmChannel => self.wasm_channel_info(name).await,
            ExtensionKind::McpServer => self.mcp_server_info(name).await,
            ExtensionKind::ChannelRelay => self.channel_relay_info(name).await,
        };
        Ok(info)
    }

    /// Detailed info for an installed WASM tool.
    async fn wasm_tool_info(&self, name: &str) -> serde_json::Value {
        let cap_path = self
            .wasm_tools_dir
            .join(format!("{}.capabilities.json", name));
        let wasm_path = self.wasm_tools_dir.join(format!("{}.wasm", name));

        let mut info = serde_json::json!({
            "name": name,
            "kind": "wasm_tool",
            "installed": wasm_path.exists(),
        });

        let versions = Self::load_capabilities(&cap_path, |bytes| {
            crate::tools::wasm::CapabilitiesFile::from_bytes(bytes).ok()
        })
        .await
        .map(|cap| (cap.version, cap.wit_version));
        apply_wasm_versions(&mut info, versions);

        info["host_wit_version"] = serde_json::json!(crate::tools::wasm::WIT_TOOL_VERSION);
        info
    }

    /// Detailed info for an installed WASM channel.
    async fn wasm_channel_info(&self, name: &str) -> serde_json::Value {
        let cap_path = self
            .wasm_channels_dir
            .join(format!("{}.capabilities.json", name));
        let wasm_path = self.wasm_channels_dir.join(format!("{}.wasm", name));

        let mut info = serde_json::json!({
            "name": name,
            "kind": "wasm_channel",
            "installed": wasm_path.exists(),
            "active": self.active_channel_names.read().await.contains(name),
        });

        let versions = Self::load_capabilities(&cap_path, |bytes| {
            crate::channels::wasm::ChannelCapabilitiesFile::from_bytes(bytes).ok()
        })
        .await
        .map(|cap| (cap.version, cap.wit_version));
        apply_wasm_versions(&mut info, versions);

        info["host_wit_version"] = serde_json::json!(crate::tools::wasm::WIT_CHANNEL_VERSION);
        info
    }

    /// Detailed info for an installed MCP server.
    async fn mcp_server_info(&self, name: &str) -> serde_json::Value {
        serde_json::json!({
            "name": name,
            "kind": "mcp_server",
            "connected": self.mcp_clients.read().await.contains_key(name),
        })
    }

    /// Detailed info for an installed channel-relay extension.
    async fn channel_relay_info(&self, name: &str) -> serde_json::Value {
        serde_json::json!({
            "name": name,
            "kind": "channel_relay",
            "active": self.active_channel_names.read().await.contains(name),
        })
    }
}

/// Fill `version` and `wit_version` from a parsed capabilities file, defaulting
/// to `"unknown"` when the file was present but a field was absent. Leaves the
/// fields untouched when no capabilities file was found.
fn apply_wasm_versions(
    info: &mut serde_json::Value,
    versions: Option<(Option<String>, Option<String>)>,
) {
    let Some((version, wit_version)) = versions else {
        return;
    };
    info["version"] = serde_json::json!(version.unwrap_or_else(|| "unknown".into()));
    info["wit_version"] = serde_json::json!(wit_version.unwrap_or_else(|| "unknown".into()));
}
