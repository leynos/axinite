//! Install extensions from the registry: build-from-source or download pre-built artifacts.

use std::path::PathBuf;

use crate::bootstrap::ironclaw_base_dir;
use crate::registry::manifest::{BundleDefinition, ExtensionManifest, ManifestKind};

mod archive;
mod artifact;
mod source;
mod validation;

#[cfg(test)]
mod tests;

/// Result of installing a single extension from the registry.
#[derive(Debug)]
pub struct InstallOutcome {
    /// Extension name.
    pub name: String,
    /// Whether this is a tool or channel.
    pub kind: ManifestKind,
    /// Destination path of the installed WASM binary.
    pub wasm_path: PathBuf,
    /// Whether a capabilities file was also installed.
    pub has_capabilities: bool,
    /// Any warning messages.
    pub warnings: Vec<String>,
}

/// Handles installing extensions from registry manifests.
pub struct RegistryInstaller {
    /// Root of the repo (parent of `registry/`), used to resolve `source.dir`.
    repo_root: PathBuf,
    /// Directory for installed tools (`~/.ironclaw/tools/`).
    tools_dir: PathBuf,
    /// Directory for installed channels (`~/.ironclaw/channels/`).
    channels_dir: PathBuf,
}

impl RegistryInstaller {
    pub fn new(repo_root: PathBuf, tools_dir: PathBuf, channels_dir: PathBuf) -> Self {
        Self {
            repo_root,
            tools_dir,
            channels_dir,
        }
    }

    /// Default installer using standard paths.
    pub fn with_defaults(repo_root: PathBuf) -> Self {
        let base_dir = ironclaw_base_dir();
        Self {
            repo_root,
            tools_dir: base_dir.join("tools"),
            channels_dir: base_dir.join("channels"),
        }
    }

    /// Install a single manifest, choosing build vs download based on artifact availability and flags.
    pub async fn install(
        &self,
        manifest: &ExtensionManifest,
        force: bool,
        prefer_build: bool,
    ) -> Result<InstallOutcome, crate::registry::catalog::RegistryError> {
        let has_artifact = manifest
            .artifacts
            .get("wasm32-wasip2")
            .and_then(|a| a.url.as_ref())
            .is_some();

        if prefer_build || !has_artifact {
            self.install_from_source(manifest, force).await
        } else {
            self.install_with_source_fallback(manifest, force).await
        }
    }

    /// Install all extensions in a bundle.
    /// Returns the outcomes and any shared auth hints.
    pub async fn install_bundle(
        &self,
        manifests: &[&ExtensionManifest],
        bundle: &BundleDefinition,
        force: bool,
        prefer_build: bool,
    ) -> (Vec<InstallOutcome>, Vec<String>) {
        let mut outcomes = Vec::new();
        let mut errors = Vec::new();

        for manifest in manifests {
            match self.install(manifest, force, prefer_build).await {
                Ok(outcome) => outcomes.push(outcome),
                Err(e) => errors.push(format!("{}: {}", manifest.name, e)),
            }
        }

        // Collect auth hints
        let mut auth_hints = Vec::new();
        if let Some(shared) = &bundle.shared_auth {
            auth_hints.push(format!(
                "Bundle uses shared auth '{}'. Run `ironclaw tool auth <any-member>` to authenticate all members.",
                shared
            ));
        }

        // Collect unique auth providers that need setup
        let mut seen_providers = std::collections::HashSet::new();
        for manifest in manifests {
            if let Some(auth) = &manifest.auth_summary {
                let key = auth
                    .shared_auth
                    .as_deref()
                    .unwrap_or(manifest.name.as_str());
                if seen_providers.insert(key.to_string())
                    && let Some(url) = &auth.setup_url
                {
                    auth_hints.push(format!(
                        "  {} ({}): {}",
                        auth.provider.as_deref().unwrap_or(&manifest.name),
                        auth.method.as_deref().unwrap_or("manual"),
                        url
                    ));
                }
            }
        }

        if !errors.is_empty() {
            auth_hints.push(format!(
                "\nFailed to install {} extension(s):",
                errors.len()
            ));
            for err in errors {
                auth_hints.push(format!("  - {}", err));
            }
        }

        (outcomes, auth_hints)
    }
}
