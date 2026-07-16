//! Extension removal and associated credential, token, and hook cleanup.

use crate::extensions::{ExtensionError, ExtensionKind};

use super::ExtensionManager;

impl ExtensionManager {
    /// Remove an installed extension.
    pub async fn remove(&self, name: &str) -> Result<String, ExtensionError> {
        Self::validate_extension_name(name)?;
        let kind = self.determine_installed_kind(name).await?;

        match kind {
            ExtensionKind::McpServer => self.remove_mcp(name).await,
            ExtensionKind::WasmTool => self.remove_wasm_tool(name).await,
            ExtensionKind::WasmChannel => self.remove_wasm_channel(name).await,
            ExtensionKind::ChannelRelay => self.remove_channel_relay(name).await,
        }
    }

    /// Remove an MCP server: unregister its tools, drop the client, and
    /// delete the config entry.
    async fn remove_mcp(&self, name: &str) -> Result<String, ExtensionError> {
        // Unregister tools with this server's prefix
        let tool_names: Vec<String> = self
            .tool_registry
            .list()
            .await
            .into_iter()
            .filter(|t| t.starts_with(&format!("{}_", name)))
            .collect();

        for tool_name in &tool_names {
            self.tool_registry.unregister(tool_name).await;
        }

        // Remove MCP client
        self.mcp_clients.write().await.remove(name);

        // Remove from config
        self.remove_mcp_server(name)
            .await
            .map_err(|e| ExtensionError::Config(e.to_string()))?;

        Ok(format!(
            "Removed MCP server '{}' and {} tool(s)",
            name,
            tool_names.len()
        ))
    }

    /// Remove a WASM tool: unregister it, revoke credentials and hooks, and
    /// delete its files.
    async fn remove_wasm_tool(&self, name: &str) -> Result<String, ExtensionError> {
        // Unregister from tool registry
        self.tool_registry.unregister(name).await;

        // Revoke credential mappings from the shared registry
        let cap_path = self
            .wasm_tools_dir
            .join(format!("{}.capabilities.json", name));
        self.revoke_credential_mappings(&cap_path, name).await;

        // Unregister hooks registered from this plugin source.
        let removed_hooks = self
            .unregister_hook_prefix(&format!("plugin.tool:{}::", name))
            .await
            + self
                .unregister_hook_prefix(&format!("plugin.dev_tool:{}::", name))
                .await;
        if removed_hooks > 0 {
            tracing::info!(
                extension = name,
                removed_hooks = removed_hooks,
                "Removed plugin hooks for WASM tool"
            );
        }

        // Delete files
        let wasm_path = self.wasm_tools_dir.join(format!("{}.wasm", name));
        Self::delete_extension_files(&wasm_path, &cap_path).await?;

        Ok(format!("Removed WASM tool '{}'", name))
    }

    /// Remove a WASM channel: deactivate it, revoke credentials, and delete
    /// its files.
    async fn remove_wasm_channel(&self, name: &str) -> Result<String, ExtensionError> {
        // Remove from active set and persist
        self.active_channel_names.write().await.remove(name);
        self.persist_active_channels().await;

        // Delete channel files
        let wasm_path = self.wasm_channels_dir.join(format!("{}.wasm", name));
        let cap_path = self
            .wasm_channels_dir
            .join(format!("{}.capabilities.json", name));

        // Revoke credential mappings before deleting the capabilities file
        self.revoke_credential_mappings(&cap_path, name).await;

        Self::delete_extension_files(&wasm_path, &cap_path).await?;

        Ok(format!(
            "Removed channel '{}'. Restart IronClaw for the change to take effect.",
            name
        ))
    }

    /// Remove a channel-relay extension: forget it, drop its token, and shut
    /// the channel down.
    async fn remove_channel_relay(&self, name: &str) -> Result<String, ExtensionError> {
        // Remove from installed set
        self.installed_relay_extensions.write().await.remove(name);

        // Remove from active channels
        self.active_channel_names.write().await.remove(name);
        self.persist_active_channels().await;

        // Remove stored stream token
        let _ = self
            .secrets
            .delete(&self.user_id, &format!("relay:{}:stream_token", name))
            .await;

        // Shut down the channel (check both runtime paths for WASM+relay and relay-only modes)
        self.shutdown_relay_channel(name).await;

        Ok(format!("Removed channel relay '{}'", name))
    }

    /// Delete an extension's `.wasm` binary (errors are fatal) and its
    /// capabilities file (best effort).
    async fn delete_extension_files(
        wasm_path: &std::path::Path,
        cap_path: &std::path::Path,
    ) -> Result<(), ExtensionError> {
        if wasm_path.exists() {
            tokio::fs::remove_file(wasm_path)
                .await
                .map_err(|e| ExtensionError::Other(e.to_string()))?;
        }
        if cap_path.exists() {
            let _ = tokio::fs::remove_file(cap_path).await;
        }
        Ok(())
    }

    /// Shut down a relay channel, preferring the WASM channel runtime and
    /// falling back to the relay-only channel manager.
    pub(super) async fn shutdown_relay_channel(&self, name: &str) {
        if let Some(ref rt) = *self.channel_runtime.read().await
            && let Some(channel) = rt.channel_manager.get_channel(name).await
        {
            let _ = channel.shutdown().await;
            return;
        }
        if let Some(ref cm) = *self.relay_channel_manager.read().await
            && let Some(channel) = cm.get_channel(name).await
        {
            let _ = channel.shutdown().await;
        }
    }

    /// Delete a tool's stored OAuth token, scopes, and refresh token so the
    /// next `auth()` call starts a fresh flow.
    ///
    /// No-op when the tool has no capabilities file or no OAuth configuration.
    pub(super) async fn purge_tool_oauth_tokens(&self, name: &str) {
        let Some(cap) = self.load_tool_capabilities(name).await else {
            return;
        };
        let Some(ref auth_cfg) = cap.auth else {
            return;
        };
        if auth_cfg.oauth.is_none() {
            return;
        }
        let secret_name = &auth_cfg.secret_name;
        let _ = self.secrets.delete(&self.user_id, secret_name).await;
        let _ = self
            .secrets
            .delete(&self.user_id, &format!("{}_scopes", secret_name))
            .await;
        let _ = self
            .secrets
            .delete(&self.user_id, &format!("{}_refresh_token", secret_name))
            .await;
    }

    /// Read a capabilities.json file and revoke owner-scoped credential
    /// mappings from the shared credential registry.
    ///
    /// The `name` parameter is the owner or extension name used to limit which
    /// mappings are removed, so removed extensions lose injection authority
    /// without affecting mappings owned by other extensions.
    pub(super) async fn revoke_credential_mappings(&self, cap_path: &std::path::Path, name: &str) {
        if !cap_path.exists() {
            return;
        }
        let Ok(bytes) = tokio::fs::read(cap_path).await else {
            return;
        };
        // Extract secret names from the capabilities JSON.
        // Structure: { "http": { "credentials": { "<key>": { "secret_name": "..." } } } }
        let Ok(json) = serde_json::from_slice::<serde_json::Value>(&bytes) else {
            return;
        };
        let secret_names: Vec<String> = json
            .get("http")
            .and_then(|h| h.get("credentials"))
            .and_then(|c| c.as_object())
            .map(|creds| {
                creds
                    .values()
                    .filter_map(|v| v.get("secret_name").and_then(|s| s.as_str()))
                    .map(String::from)
                    .collect()
            })
            .unwrap_or_default();

        if secret_names.is_empty() {
            return;
        }

        if let Some(cr) = self.tool_registry.credential_registry() {
            cr.remove_mappings_for_secrets(name, &secret_names);
            tracing::info!(
                owner = name,
                secrets = ?secret_names,
                "Revoked credential mappings for removed extension"
            );
        }
    }

    pub(super) async fn unregister_hook_prefix(&self, prefix: &str) -> usize {
        let Some(ref hooks) = self.hooks else {
            return 0;
        };

        let names = hooks.list().await;
        let mut removed = 0;
        for hook_name in names {
            if hook_name.starts_with(prefix) && hooks.unregister(&hook_name).await {
                removed += 1;
            }
        }
        removed
    }
}
