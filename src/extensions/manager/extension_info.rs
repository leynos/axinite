//! Detailed per-extension info reporting for the extension manager.

use crate::extensions::{ExtensionError, ExtensionKind};

use super::ExtensionManager;

impl ExtensionManager {
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
        let versions = Self::load_capabilities(&cap_path, |bytes| {
            crate::tools::wasm::CapabilitiesFile::from_bytes(bytes).ok()
        })
        .await
        .map(|cap| (cap.version, cap.wit_version));

        Self::wasm_extension_info(
            name,
            &self.wasm_tools_dir,
            "wasm_tool",
            None,
            crate::tools::wasm::WIT_TOOL_VERSION,
            versions,
        )
    }

    /// Detailed info for an installed WASM channel.
    async fn wasm_channel_info(&self, name: &str) -> serde_json::Value {
        let cap_path = self
            .wasm_channels_dir
            .join(format!("{}.capabilities.json", name));
        let versions = Self::load_capabilities(&cap_path, |bytes| {
            crate::channels::wasm::ChannelCapabilitiesFile::from_bytes(bytes).ok()
        })
        .await
        .map(|cap| (cap.version, cap.wit_version));
        let active = self.active_channel_names.read().await.contains(name);

        Self::wasm_extension_info(
            name,
            &self.wasm_channels_dir,
            "wasm_channel",
            Some(active),
            crate::tools::wasm::WIT_CHANNEL_VERSION,
            versions,
        )
    }

    /// Assemble the shared JSON info object for a discovered WASM extension.
    ///
    /// `active` is `Some` only for channels; tools omit the `active` field.
    /// `versions` carries the optional `(version, wit_version)` pair parsed
    /// from the extension's capabilities file.
    fn wasm_extension_info(
        name: &str,
        dir: &std::path::Path,
        kind: &str,
        active: Option<bool>,
        host_wit_version: &str,
        versions: Option<(Option<String>, Option<String>)>,
    ) -> serde_json::Value {
        let wasm_path = dir.join(format!("{}.wasm", name));
        let mut info = serde_json::json!({
            "name": name,
            "kind": kind,
            "installed": wasm_path.exists(),
        });
        if let Some(active) = active {
            info["active"] = serde_json::json!(active);
        }
        apply_wasm_versions(&mut info, versions);
        info["host_wit_version"] = serde_json::json!(host_wit_version);
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
