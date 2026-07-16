//! Catalog construction: locating the registry directory and loading
//! manifests and bundles from disk or the embedded fallback.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use crate::registry::embedded;
use crate::registry::manifest::{BundlesFile, ExtensionManifest};

use super::{RegistryCatalog, RegistryError};

/// Return `dir/registry` when it exists as a directory.
fn registry_dir_under(dir: &Path) -> Option<PathBuf> {
    let candidate = dir.join("registry");
    candidate.is_dir().then_some(candidate)
}

/// Look for `registry/` relative to the current directory (for dev usage).
fn registry_dir_near_cwd() -> Option<PathBuf> {
    let cwd = std::env::current_dir().ok()?;
    registry_dir_under(&cwd)
}

/// Look for `registry/` relative to the executable, walking up to three
/// levels (covers an installed binary, `target/debug/`, and `target/release/`
/// back to the repo root).
fn registry_dir_near_exe() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    let mut dir = exe.parent();
    for _ in 0..3 {
        let d = dir?;
        if let Some(found) = registry_dir_under(d) {
            return Some(found);
        }
        dir = d.parent();
    }
    None
}

/// Look for `registry/` under `CARGO_MANIFEST_DIR` (compile-time, works in
/// dev builds).
fn registry_dir_in_manifest_dir() -> Option<PathBuf> {
    registry_dir_under(Path::new(env!("CARGO_MANIFEST_DIR")))
}

impl RegistryCatalog {
    /// Find the `registry/` directory by searching relative to cwd, the executable,
    /// and `CARGO_MANIFEST_DIR`. Returns `None` if the directory cannot be found
    /// (non-fatal at startup).
    pub fn find_dir() -> Option<PathBuf> {
        registry_dir_near_cwd()
            .or_else(registry_dir_near_exe)
            .or_else(registry_dir_in_manifest_dir)
    }

    /// Try to load from disk; if `registry/` cannot be found, fall back to
    /// manifests embedded into the binary at compile time.
    pub fn load_or_embedded() -> Result<Self, RegistryError> {
        if let Some(dir) = Self::find_dir() {
            return Self::load(&dir);
        }

        // Fall back to embedded catalog
        let manifests = embedded::load_embedded();
        let bundles = embedded::load_embedded_bundles();

        tracing::info!(
            "Loaded embedded registry catalog ({} extensions, {} bundles)",
            manifests.len(),
            bundles.len()
        );

        Ok(Self {
            manifests,
            bundles,
            root: PathBuf::new(),
        })
    }

    /// Load the catalog from a registry directory.
    ///
    /// Expects the structure:
    /// ```text
    /// registry/
    /// ├── tools/*.json
    /// ├── channels/*.json
    /// └── _bundles.json
    /// ```
    pub fn load(registry_dir: &Path) -> Result<Self, RegistryError> {
        if !registry_dir.exists() {
            return Err(RegistryError::DirectoryNotFound(registry_dir.to_path_buf()));
        }

        let mut manifests = HashMap::new();

        // Load tools
        let tools_dir = registry_dir.join("tools");
        if tools_dir.is_dir() {
            Self::load_manifests_from_dir(&tools_dir, "tools", &mut manifests)?;
        }

        // Load channels
        let channels_dir = registry_dir.join("channels");
        if channels_dir.is_dir() {
            Self::load_manifests_from_dir(&channels_dir, "channels", &mut manifests)?;
        }

        // Load bundles
        let bundles_path = registry_dir.join("_bundles.json");
        let bundles = if bundles_path.is_file() {
            let content = ambient_fs::read_to_string(&bundles_path).map_err(|e| {
                RegistryError::BundlesRead(format!("{}: {}", bundles_path.display(), e))
            })?;
            let bundles_file: BundlesFile = serde_json::from_str(&content).map_err(|e| {
                RegistryError::BundlesRead(format!("{}: {}", bundles_path.display(), e))
            })?;
            bundles_file.bundles
        } else {
            HashMap::new()
        };

        Ok(Self {
            manifests,
            bundles,
            root: registry_dir.to_path_buf(),
        })
    }

    fn load_manifests_from_dir(
        dir: &Path,
        kind_prefix: &str,
        manifests: &mut HashMap<String, ExtensionManifest>,
    ) -> Result<(), RegistryError> {
        let entries = ambient_fs::read_dir(dir).map_err(|e| RegistryError::ManifestRead {
            path: dir.to_path_buf(),
            reason: e.to_string(),
        })?;

        for entry in entries {
            let entry = entry.map_err(|e| RegistryError::ManifestRead {
                path: dir.to_path_buf(),
                reason: e.to_string(),
            })?;

            let path = entry.path();
            if !path.is_file() || path.extension().and_then(|e| e.to_str()) != Some("json") {
                continue;
            }

            let content =
                ambient_fs::read_to_string(&path).map_err(|e| RegistryError::ManifestRead {
                    path: path.clone(),
                    reason: e.to_string(),
                })?;

            let manifest: ExtensionManifest =
                serde_json::from_str(&content).map_err(|e| RegistryError::ManifestParse {
                    path: path.clone(),
                    reason: e.to_string(),
                })?;

            let key = format!("{}/{}", kind_prefix, manifest.name);
            manifests.insert(key, manifest);
        }

        Ok(())
    }
}
