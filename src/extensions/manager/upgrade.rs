//! WASM extension upgrade flow (binary and capabilities replacement).

use crate::extensions::{
    ExtensionError, ExtensionKind, RegistryEntry, UpgradeOutcome, UpgradeResult,
};
use crate::tools::wasm::discover_tools;

use super::ExtensionManager;

/// Per-extension context shared by the phases of a single upgrade attempt.
///
/// Bundles the extension identity with its declared and host WIT versions so
/// each phase can build its outcome without re-threading the same arguments.
struct UpgradeContext<'a> {
    name: &'a str,
    kind: ExtensionKind,
    declared_wit: Option<String>,
    host_wit: &'a str,
}

impl UpgradeContext<'_> {
    /// The declared WIT version, or `"unknown"` when the capabilities file was
    /// absent or unreadable.
    fn declared_or_unknown(&self) -> &str {
        self.declared_wit.as_deref().unwrap_or("unknown")
    }

    /// Whether the declared WIT version is incompatible with the host and thus
    /// warrants an upgrade.
    fn needs_upgrade(&self) -> bool {
        crate::tools::wasm::check_wit_version_compat(
            self.name,
            self.declared_wit.as_deref(),
            self.host_wit,
        )
        .is_err()
    }

    /// Build an [`UpgradeOutcome`] for this extension, tagging the shared name
    /// and kind so each phase supplies only its status and detail.
    fn make_outcome(&self, status: &str, detail: String) -> UpgradeOutcome {
        outcome(self.name, self.kind, status, detail)
    }

    fn up_to_date_outcome(&self) -> UpgradeOutcome {
        self.make_outcome(
            "already_up_to_date",
            format!(
                "WIT {} matches host WIT {}",
                self.declared_or_unknown(),
                self.host_wit
            ),
        )
    }

    fn not_in_registry_outcome(&self) -> UpgradeOutcome {
        self.make_outcome(
            "not_in_registry",
            format!(
                concat!(
                    "Extension '{}' has outdated WIT {} (host: {}), ",
                    "but is not in the registry. Reinstall manually with a URL."
                ),
                self.name,
                self.declared_or_unknown(),
                self.host_wit
            ),
        )
    }

    fn upgraded_outcome(&self) -> UpgradeOutcome {
        self.make_outcome(
            "upgraded",
            format!(
                "Upgraded from WIT {} to host WIT {}. Restart to activate.",
                self.declared_or_unknown(),
                self.host_wit
            ),
        )
    }

    /// Build a `"failed"` outcome with a human-readable reason.
    fn failed_outcome(&self, detail: String) -> UpgradeOutcome {
        self.make_outcome("failed", detail)
    }
}

/// Discover installed WASM extensions of one kind as upgrade candidates.
///
/// Yields an empty list when the directory is absent or discovery fails. The
/// tool and channel discovery functions return different concrete map types,
/// so a macro (rather than a generic fn) keeps both call sites identical.
macro_rules! discover_candidates {
    ($dir:expr, $discover:expr, $kind:expr) => {{
        if !$dir.exists() {
            Vec::new()
        } else {
            match $discover.await {
                Ok(items) => items.into_keys().map(|name| (name, $kind)).collect(),
                Err(_) => Vec::new(),
            }
        }
    }};
}

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
        let Some(name) = name else {
            return Ok(self.discover_all_candidates().await);
        };
        self.resolve_named_candidate(name).await
    }

    /// Validate a named extension and confirm it is upgradeable (not an MCP
    /// server, which has no WIT version).
    async fn resolve_named_candidate(
        &self,
        name: &str,
    ) -> Result<Vec<(String, ExtensionKind)>, ExtensionError> {
        Self::validate_extension_name(name)?;
        let kind = self.determine_installed_kind(name).await?;
        if kind == ExtensionKind::McpServer {
            return Err(ExtensionError::Other(
                "MCP servers don't have WIT versions and cannot be upgraded this way".to_string(),
            ));
        }
        Ok(vec![(name.to_string(), kind)])
    }

    /// Discover every installed WASM tool and channel as an upgrade candidate.
    async fn discover_all_candidates(&self) -> Vec<(String, ExtensionKind)> {
        let mut candidates = self.discover_tool_candidates().await;
        candidates.extend(self.discover_channel_candidates().await);
        candidates
    }

    /// Discover installed WASM tools; empty when the directory is absent or
    /// discovery fails.
    async fn discover_tool_candidates(&self) -> Vec<(String, ExtensionKind)> {
        discover_candidates!(
            self.wasm_tools_dir,
            discover_tools(&self.wasm_tools_dir),
            ExtensionKind::WasmTool
        )
    }

    /// Discover installed WASM channels; empty when the directory is absent or
    /// discovery fails.
    async fn discover_channel_candidates(&self) -> Vec<(String, ExtensionKind)> {
        discover_candidates!(
            self.wasm_channels_dir,
            crate::channels::wasm::discover_channels(&self.wasm_channels_dir),
            ExtensionKind::WasmChannel
        )
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
    ///
    /// The flow runs in named phases: resolve the target directory and host
    /// WIT version, read the declared version, decide whether an upgrade is
    /// needed, look up a registry entry, then reinstall while preserving
    /// secrets.
    pub(super) async fn upgrade_one(&self, name: &str, kind: ExtensionKind) -> UpgradeOutcome {
        let Some((cap_dir, host_wit)) = self.resolve_upgrade_target(kind) else {
            return outcome(
                name,
                kind,
                "failed",
                "This extension type cannot be upgraded this way".to_string(),
            );
        };

        // Read the current WIT version from the capabilities file.
        let cap_path = cap_dir.join(format!("{}.capabilities.json", name));
        let declared_wit = Self::declared_wit_version(&cap_path, kind).await;

        let ctx = UpgradeContext {
            name,
            kind,
            declared_wit,
            host_wit,
        };

        if !ctx.needs_upgrade() {
            return ctx.up_to_date_outcome();
        }

        let Some(entry) = self.registry.get_with_kind(name, Some(kind)).await else {
            return ctx.not_in_registry_outcome();
        };

        self.reinstall_upgraded(&ctx, cap_dir, &entry).await
    }

    /// Resolve the capabilities directory and host WIT version for an
    /// upgradeable extension kind, or `None` for kinds without a WIT version.
    fn resolve_upgrade_target(
        &self,
        kind: ExtensionKind,
    ) -> Option<(&std::path::Path, &'static str)> {
        match kind {
            ExtensionKind::WasmTool => Some((
                self.wasm_tools_dir.as_path(),
                crate::tools::wasm::WIT_TOOL_VERSION,
            )),
            ExtensionKind::WasmChannel => Some((
                self.wasm_channels_dir.as_path(),
                crate::tools::wasm::WIT_CHANNEL_VERSION,
            )),
            ExtensionKind::McpServer | ExtensionKind::ChannelRelay => None,
        }
    }

    /// Remove the stale `.wasm` (and capabilities) files and reinstall from the
    /// registry entry, keeping authentication secrets intact.
    async fn reinstall_upgraded(
        &self,
        ctx: &UpgradeContext<'_>,
        cap_dir: &std::path::Path,
        entry: &RegistryEntry,
    ) -> UpgradeOutcome {
        // Delete old .wasm file (keep secrets intact).
        let cap_path = cap_dir.join(format!("{}.capabilities.json", ctx.name));
        let wasm_path = cap_dir.join(format!("{}.wasm", ctx.name));
        if wasm_path.exists()
            && let Err(e) = tokio::fs::remove_file(&wasm_path).await
        {
            return ctx.failed_outcome(format!("Failed to remove old WASM binary: {}", e));
        }
        // Also remove old capabilities so install_from_entry can write the new one.
        if cap_path.exists() {
            let _ = tokio::fs::remove_file(&cap_path).await;
        }

        match self.install_from_entry(entry).await {
            Ok(_) => {
                tracing::info!(
                    extension = %ctx.name,
                    old_wit = ?ctx.declared_wit,
                    new_host_wit = %ctx.host_wit,
                    "Upgraded WASM extension"
                );
                ctx.upgraded_outcome()
            }
            Err(e) => {
                ctx.failed_outcome(format!("Reinstall failed: {}. Old files were removed.", e))
            }
        }
    }
}
