//! WASM extension upgrade flow (binary and capabilities replacement).

use crate::extensions::{ExtensionError, ExtensionKind, UpgradeOutcome, UpgradeResult};
use crate::tools::wasm::discover_tools;

use super::ExtensionManager;

/// Build an [`UpgradeOutcome`] with the given status and detail.
fn outcome(name: &str, kind: ExtensionKind, status: &str, detail: String) -> UpgradeOutcome {
    UpgradeOutcome {
        name: name.to_string(),
        kind,
        status: status.to_string(),
        detail,
    }
}

/// Summarize a batch of upgrade outcomes for the user.
fn summarize_outcomes(outcomes: &[UpgradeOutcome]) -> String {
    let upgraded = outcomes.iter().filter(|o| o.status == "upgraded").count();
    let up_to_date = outcomes
        .iter()
        .filter(|o| o.status == "already_up_to_date")
        .count();
    let failed = outcomes.iter().filter(|o| o.status == "failed").count();

    format!(
        "{} extension(s) checked: {} upgraded, {} already up to date, {} failed",
        outcomes.len(),
        upgraded,
        up_to_date,
        failed
    )
}

impl ExtensionManager {
    /// Upgrade installed WASM extensions to match the current host WIT version.
    ///
    /// If `name` is `Some`, upgrades only that extension.  If `None`, checks all
    /// installed WASM tools and channels and upgrades any that are outdated.
    ///
    /// The upgrade preserves authentication secrets — only the `.wasm` binary
    /// (and `.capabilities.json`) are replaced.
    pub async fn upgrade(&self, name: Option<&str>) -> Result<UpgradeResult, ExtensionError> {
        let candidates = self.collect_upgrade_candidates(name).await?;

        if candidates.is_empty() {
            return Ok(UpgradeResult {
                results: Vec::new(),
                message: "No WASM extensions installed.".to_string(),
            });
        }

        let mut outcomes = Vec::new();
        for (ext_name, kind) in &candidates {
            outcomes.push(self.upgrade_one(ext_name, *kind).await);
        }

        let message = summarize_outcomes(&outcomes);
        Ok(UpgradeResult {
            results: outcomes,
            message,
        })
    }

    /// Resolve which extensions to check: the named one, or all discovered
    /// WASM tools and channels.
    async fn collect_upgrade_candidates(
        &self,
        name: Option<&str>,
    ) -> Result<Vec<(String, ExtensionKind)>, ExtensionError> {
        let mut candidates: Vec<(String, ExtensionKind)> = Vec::new();

        if let Some(name) = name {
            Self::validate_extension_name(name)?;
            let kind = self.determine_installed_kind(name).await?;
            if kind == ExtensionKind::McpServer {
                return Err(ExtensionError::Other(
                    "MCP servers don't have WIT versions and cannot be upgraded this way"
                        .to_string(),
                ));
            }
            candidates.push((name.to_string(), kind));
            return Ok(candidates);
        }

        // Discover all installed WASM tools
        if self.wasm_tools_dir.exists()
            && let Ok(tools) = discover_tools(&self.wasm_tools_dir).await
        {
            for (tool_name, _) in tools {
                candidates.push((tool_name, ExtensionKind::WasmTool));
            }
        }
        // Discover all installed WASM channels
        if self.wasm_channels_dir.exists()
            && let Ok(channels) =
                crate::channels::wasm::discover_channels(&self.wasm_channels_dir).await
        {
            for (ch_name, _) in channels {
                candidates.push((ch_name, ExtensionKind::WasmChannel));
            }
        }
        Ok(candidates)
    }

    /// Read the WIT version declared in an extension's capabilities file.
    async fn declared_wit_version(
        cap_path: &std::path::Path,
        kind: ExtensionKind,
    ) -> Option<String> {
        if !cap_path.exists() {
            return None;
        }
        let bytes = tokio::fs::read(cap_path).await.ok()?;
        match kind {
            ExtensionKind::WasmTool => crate::tools::wasm::CapabilitiesFile::from_bytes(&bytes)
                .ok()
                .and_then(|c| c.wit_version),
            ExtensionKind::WasmChannel => {
                crate::channels::wasm::ChannelCapabilitiesFile::from_bytes(&bytes)
                    .ok()
                    .and_then(|c| c.wit_version)
            }
            ExtensionKind::McpServer | ExtensionKind::ChannelRelay => None,
        }
    }

    /// Upgrade a single WASM extension if its WIT version is outdated.
    pub(super) async fn upgrade_one(&self, name: &str, kind: ExtensionKind) -> UpgradeOutcome {
        let (cap_dir, host_wit) = match kind {
            ExtensionKind::WasmTool => (&self.wasm_tools_dir, crate::tools::wasm::WIT_TOOL_VERSION),
            ExtensionKind::WasmChannel => (
                &self.wasm_channels_dir,
                crate::tools::wasm::WIT_CHANNEL_VERSION,
            ),
            ExtensionKind::McpServer | ExtensionKind::ChannelRelay => {
                return outcome(
                    name,
                    kind,
                    "failed",
                    "This extension type cannot be upgraded this way".to_string(),
                );
            }
        };

        // Read current WIT version from capabilities
        let cap_path = cap_dir.join(format!("{}.capabilities.json", name));
        let declared_wit = Self::declared_wit_version(&cap_path, kind).await;

        // Check if upgrade is needed
        let needs_upgrade =
            crate::tools::wasm::check_wit_version_compat(name, declared_wit.as_deref(), host_wit)
                .is_err();

        if !needs_upgrade {
            return outcome(
                name,
                kind,
                "already_up_to_date",
                format!(
                    "WIT {} matches host WIT {}",
                    declared_wit.as_deref().unwrap_or("unknown"),
                    host_wit
                ),
            );
        }

        // Check registry for a newer version
        let entry = self.registry.get_with_kind(name, Some(kind)).await;
        let Some(entry) = entry else {
            return outcome(
                name,
                kind,
                "not_in_registry",
                format!(
                    "Extension '{}' has outdated WIT {} (host: {}), \
                     but is not in the registry. Reinstall manually with a URL.",
                    name,
                    declared_wit.as_deref().unwrap_or("unknown"),
                    host_wit
                ),
            );
        };

        // Delete old .wasm file (keep secrets intact)
        let wasm_path = cap_dir.join(format!("{}.wasm", name));
        if wasm_path.exists()
            && let Err(e) = tokio::fs::remove_file(&wasm_path).await
        {
            return outcome(
                name,
                kind,
                "failed",
                format!("Failed to remove old WASM binary: {}", e),
            );
        }
        // Also remove old capabilities so install_from_entry can write the new one
        if cap_path.exists() {
            let _ = tokio::fs::remove_file(&cap_path).await;
        }

        // Reinstall from registry
        match self.install_from_entry(&entry).await {
            Ok(_) => {
                tracing::info!(
                    extension = %name,
                    old_wit = ?declared_wit,
                    new_host_wit = %host_wit,
                    "Upgraded WASM extension"
                );
                outcome(
                    name,
                    kind,
                    "upgraded",
                    format!(
                        "Upgraded from WIT {} to host WIT {}. Restart to activate.",
                        declared_wit.as_deref().unwrap_or("unknown"),
                        host_wit
                    ),
                )
            }
            Err(e) => outcome(
                name,
                kind,
                "failed",
                format!("Reinstall failed: {}. Old files were removed.", e),
            ),
        }
    }
}
