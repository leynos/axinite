//! Build-from-source installation path: compiles the extension's WASM
//! component from the repo checkout and copies it into place.

use tokio::fs;

use crate::registry::catalog::RegistryError;
use crate::registry::manifest::{ExtensionManifest, ManifestKind};

use super::validation::validate_manifest_install_inputs;
use super::{InstallOutcome, RegistryInstaller};

impl RegistryInstaller {
    /// Install a single extension by building from source.
    pub async fn install_from_source(
        &self,
        manifest: &ExtensionManifest,
        force: bool,
    ) -> Result<InstallOutcome, RegistryError> {
        validate_manifest_install_inputs(manifest)?;

        let source_dir = self.repo_root.join(&manifest.source.dir);
        if !source_dir.exists() {
            return Err(RegistryError::ManifestRead {
                path: source_dir.clone(),
                reason: "source directory does not exist".to_string(),
            });
        }

        let target_dir = match manifest.kind {
            ManifestKind::Tool => &self.tools_dir,
            ManifestKind::Channel => &self.channels_dir,
        };

        fs::create_dir_all(target_dir)
            .await
            .map_err(RegistryError::Io)?;

        // Use manifest.name for installed filenames so discovery, auth, and
        // CLI commands (`ironclaw tool auth <name>`) all agree on the stem.
        let target_wasm = target_dir.join(format!("{}.wasm", manifest.name));

        // Check if already exists
        if target_wasm.exists() && !force {
            return Err(RegistryError::AlreadyInstalled {
                name: manifest.name.clone(),
                path: target_wasm,
            });
        }

        // Build the WASM component
        println!(
            "Building {} '{}' from {}...",
            manifest.kind,
            manifest.display_name,
            source_dir.display()
        );
        let crate_name = &manifest.source.crate_name;
        let wasm_path =
            crate::registry::artifacts::build_wasm_component(&source_dir, crate_name, true)
                .await
                .map_err(|e| RegistryError::ManifestRead {
                    path: source_dir.clone(),
                    reason: format!("build failed: {}", e),
                })?;

        // Copy WASM binary
        println!("  Installing to {}", target_wasm.display());
        fs::copy(&wasm_path, &target_wasm)
            .await
            .map_err(RegistryError::Io)?;

        // Copy capabilities file
        let caps_source = source_dir.join(&manifest.source.capabilities);
        let target_caps = target_dir.join(format!("{}.capabilities.json", manifest.name));
        let has_capabilities = if caps_source.exists() {
            fs::copy(&caps_source, &target_caps)
                .await
                .map_err(RegistryError::Io)?;
            true
        } else {
            false
        };

        let mut warnings = Vec::new();
        if !has_capabilities {
            warnings.push(format!(
                "No capabilities file found at {}",
                caps_source.display()
            ));
        }

        Ok(InstallOutcome {
            name: manifest.name.clone(),
            kind: manifest.kind,
            wasm_path: target_wasm,
            has_capabilities,
            warnings,
        })
    }
}
