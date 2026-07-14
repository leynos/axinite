//! Build-from-source installation path: compiles the extension's WASM
//! component from the repo checkout and copies it into place.

use std::path::PathBuf;

use tokio::fs;

use crate::registry::catalog::RegistryError;
use crate::registry::manifest::{ExtensionManifest, ManifestKind};

use super::validation::validate_manifest_install_inputs;
use super::{InstallOutcome, RegistryInstaller};

impl RegistryInstaller {
    /// Prepare the install target for a manifest: pick the tools or channels
    /// directory, create it, and return the target `.wasm` path.
    ///
    /// Uses `manifest.name` for installed filenames so discovery, auth, and
    /// CLI commands (`ironclaw tool auth <name>`) all agree on the stem.
    /// Fails with `AlreadyInstalled` when the target exists and `force` is
    /// not set.
    pub(super) async fn prepare_install_target(
        &self,
        manifest: &ExtensionManifest,
        force: bool,
    ) -> Result<PathBuf, RegistryError> {
        let target_dir = match manifest.kind {
            ManifestKind::Tool => &self.tools_dir,
            ManifestKind::Channel => &self.channels_dir,
        };

        fs::create_dir_all(target_dir)
            .await
            .map_err(RegistryError::Io)?;

        let target_wasm = target_dir.join(format!("{}.wasm", manifest.name));

        if target_wasm.exists() && !force {
            return Err(RegistryError::AlreadyInstalled {
                name: manifest.name.clone(),
                path: target_wasm,
            });
        }

        Ok(target_wasm)
    }

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

        let target_wasm = self.prepare_install_target(manifest, force).await?;

        build_and_install_wasm(manifest, &source_dir, &target_wasm).await?;

        let (has_capabilities, warnings) =
            copy_capabilities_from_source(manifest, &source_dir, &target_wasm).await?;

        Ok(InstallOutcome {
            name: manifest.name.clone(),
            kind: manifest.kind,
            wasm_path: target_wasm,
            has_capabilities,
            warnings,
        })
    }
}

/// Build the WASM component from the source checkout and copy it to the
/// target path.
async fn build_and_install_wasm(
    manifest: &ExtensionManifest,
    source_dir: &std::path::Path,
    target_wasm: &std::path::Path,
) -> Result<(), RegistryError> {
    println!(
        "Building {} '{}' from {}...",
        manifest.kind,
        manifest.display_name,
        source_dir.display()
    );
    let crate_name = &manifest.source.crate_name;
    let wasm_path = crate::registry::artifacts::build_wasm_component(source_dir, crate_name, true)
        .await
        .map_err(|e| RegistryError::ManifestRead {
            path: source_dir.to_path_buf(),
            reason: format!("build failed: {}", e),
        })?;

    println!("  Installing to {}", target_wasm.display());
    fs::copy(&wasm_path, target_wasm)
        .await
        .map_err(RegistryError::Io)?;
    Ok(())
}

/// Copy the capabilities file from the source checkout next to the installed
/// WASM binary. Returns whether capabilities were found, plus any warnings.
async fn copy_capabilities_from_source(
    manifest: &ExtensionManifest,
    source_dir: &std::path::Path,
    target_wasm: &std::path::Path,
) -> Result<(bool, Vec<String>), RegistryError> {
    let caps_source = source_dir.join(&manifest.source.capabilities);
    let target_caps = target_wasm.with_file_name(format!("{}.capabilities.json", manifest.name));

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

    Ok((has_capabilities, warnings))
}
