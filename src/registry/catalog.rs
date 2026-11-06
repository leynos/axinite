//! Registry catalog: loads manifests from disk, provides list/search/resolve operations.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use crate::registry::manifest::{BundleDefinition, ExtensionManifest, ManifestKind};

mod load;

/// Error type for registry operations.
#[derive(Debug, thiserror::Error)]
pub enum RegistryError {
    /// The registry directory does not exist. Carries the path that was tried.
    #[error("Registry directory not found: {0}")]
    DirectoryNotFound(PathBuf),

    /// A manifest file could not be read from disk.
    #[error("Failed to read manifest {path}: {reason}")]
    ManifestRead {
        /// Manifest file that could not be read.
        path: PathBuf,
        /// Underlying read error message.
        reason: String,
    },

    /// A manifest file was read but is not valid JSON for the manifest schema.
    #[error("Failed to parse manifest {path}: {reason}")]
    ManifestParse {
        /// Manifest file that failed to parse.
        path: PathBuf,
        /// Parser error message.
        reason: String,
    },

    /// No extension matches the requested name, as given by the caller.
    #[error("Extension not found: {0}")]
    ExtensionNotFound(String),

    /// The extension is already installed and `force` was not set.
    #[error("'{name}' already installed at {path}. Use --force to overwrite.")]
    AlreadyInstalled {
        /// Extension name.
        name: String,
        /// Location of the existing install.
        path: std::path::PathBuf,
    },

    // `url` is stored for programmatic access (logs, retries) but intentionally
    // omitted from the Display message to avoid leaking internal artifact URLs
    // to end users.
    /// Fetching a pre-built artefact or its capabilities file failed.
    #[error("Artifact download failed: {reason}")]
    DownloadFailed {
        /// Artefact URL that was requested.
        url: String,
        /// Why the request failed, such as an HTTP status or a body read error.
        reason: String,
    },

    /// A manifest field failed validation, such as an unparseable or disallowed URL.
    #[error("Invalid extension manifest for '{name}' field '{field}': {reason}")]
    InvalidManifest {
        /// Extension name.
        name: String,
        /// Dotted manifest path of the rejected field, such as `artifacts.wasm32-wasip2.url`.
        field: &'static str,
        /// Why the value was rejected.
        reason: String,
    },

    /// The downloaded artefact's SHA-256 digest differs from the manifest value.
    #[error("Checksum verification failed: expected {expected_sha256}, got {actual_sha256}")]
    ChecksumMismatch {
        /// Artefact URL the bytes were downloaded from.
        url: String,
        /// Hex digest declared in the manifest.
        expected_sha256: String,
        /// Hex digest of the downloaded bytes.
        actual_sha256: String,
    },

    /// The manifest declares an artefact without a `sha256` value.
    #[error("Missing SHA256 checksum for '{name}' artifact. Use --build to build from source.")]
    MissingChecksum {
        /// Extension whose artefact has no checksum.
        name: String,
    },

    /// The artefact install failed and the source fallback cannot run because the
    /// source directory is missing.
    #[error(
        "Source fallback unavailable for '{name}' after artifact install failed. Retry artifact download or run from a repository checkout."
    )]
    SourceFallbackUnavailable {
        /// Extension name.
        name: String,
        /// In-repository source directory the fallback would build from.
        source_dir: PathBuf,
        /// The error from the failed artefact install.
        artifact_error: Box<RegistryError>,
    },

    /// The artefact install and the source-build fallback both failed.
    #[error("Artifact install and source fallback both failed for '{name}'.")]
    InstallFallbackFailed {
        /// Extension name.
        name: String,
        /// The error from the failed artefact install.
        artifact_error: Box<RegistryError>,
        /// The error from the failed source build.
        source_error: Box<RegistryError>,
    },

    /// A bare name matches both a tool and a channel; the message suggests qualified names.
    #[error(
        "Ambiguous name '{name}': exists as both {kind_a} and {kind_b}. Use '{prefix_a}/{name}' or '{prefix_b}/{name}'."
    )]
    AmbiguousName {
        /// The ambiguous bare name.
        name: String,
        /// Kind label of the first match, `tool`.
        kind_a: &'static str,
        /// Qualifying key prefix of the first match, `tools`.
        prefix_a: &'static str,
        /// Kind label of the second match, `channel`.
        kind_b: &'static str,
        /// Qualifying key prefix of the second match, `channels`.
        prefix_b: &'static str,
    },

    /// No bundle with this name is defined in `_bundles.json`.
    #[error("Bundle not found: {0}")]
    BundleNotFound(String),

    /// `_bundles.json` could not be read or parsed. Carries the file path and error.
    #[error("Failed to read bundles file: {0}")]
    BundlesRead(String),

    /// Filesystem error from the catalog or installer; converted from `std::io::Error`.
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
}

/// Central catalog loaded from the `registry/` directory.
#[derive(Debug, Clone)]
pub struct RegistryCatalog {
    /// All loaded manifests, keyed by "<kind>/<name>" (e.g. "tools/github").
    manifests: HashMap<String, ExtensionManifest>,

    /// Bundle definitions from `_bundles.json`.
    bundles: HashMap<String, BundleDefinition>,

    /// Root directory of the registry.
    root: PathBuf,
}

impl RegistryCatalog {
    /// The root directory this catalog was loaded from.
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Get all manifests.
    pub fn all(&self) -> Vec<&ExtensionManifest> {
        let mut items: Vec<_> = self.manifests.values().collect();
        items.sort_by(|a, b| a.name.cmp(&b.name));
        items
    }

    /// List manifests, optionally filtered by kind and/or tag.
    pub fn list(&self, kind: Option<ManifestKind>, tag: Option<&str>) -> Vec<&ExtensionManifest> {
        let mut results: Vec<_> = self
            .manifests
            .values()
            .filter(|m| kind.is_none_or(|k| m.kind == k))
            .filter(|m| tag.is_none_or(|t| m.tags.iter().any(|mt| mt == t)))
            .collect();
        results.sort_by(|a, b| a.name.cmp(&b.name));
        results
    }

    /// Get a manifest by name. Tries exact key match first ("tools/github"),
    /// then searches by bare name ("github").
    ///
    /// If a bare name matches both a tool and a channel, returns `None`.
    /// Use a qualified key ("tools/github" or "channels/telegram") to disambiguate.
    pub fn get(&self, name: &str) -> Option<&ExtensionManifest> {
        // Try exact key first
        if let Some(m) = self.manifests.get(name) {
            return Some(m);
        }

        // Try with kind prefix, detecting collisions
        let tool = self.manifests.get(&format!("tools/{}", name));
        let channel = self.manifests.get(&format!("channels/{}", name));

        match (tool, channel) {
            (Some(_), Some(_)) => None, // ambiguous
            (Some(m), None) => Some(m),
            (None, Some(m)) => Some(m),
            (None, None) => None,
        }
    }

    /// Get a manifest by name, returning a `Result` with an explicit error for
    /// ambiguous bare names.
    pub fn get_strict(&self, name: &str) -> Result<&ExtensionManifest, RegistryError> {
        // Try exact key first
        if let Some(m) = self.manifests.get(name) {
            return Ok(m);
        }

        let has_tool = self.manifests.contains_key(&format!("tools/{}", name));
        let has_channel = self.manifests.contains_key(&format!("channels/{}", name));

        match (has_tool, has_channel) {
            (true, true) => Err(RegistryError::AmbiguousName {
                name: name.to_string(),
                kind_a: "tool",
                prefix_a: "tools",
                kind_b: "channel",
                prefix_b: "channels",
            }),
            (true, false) => Ok(self.manifests.get(&format!("tools/{}", name)).unwrap()),
            (false, true) => Ok(self.manifests.get(&format!("channels/{}", name)).unwrap()),
            (false, false) => Err(RegistryError::ExtensionNotFound(name.to_string())),
        }
    }

    /// Get the full key ("tools/github" or "channels/telegram") for a manifest.
    pub fn key_for(&self, name: &str) -> Option<String> {
        if self.manifests.contains_key(name) {
            return Some(name.to_string());
        }

        let has_tool = self.manifests.contains_key(&format!("tools/{}", name));
        let has_channel = self.manifests.contains_key(&format!("channels/{}", name));

        match (has_tool, has_channel) {
            (true, true) => None, // ambiguous
            (true, false) => Some(format!("tools/{}", name)),
            (false, true) => Some(format!("channels/{}", name)),
            (false, false) => None,
        }
    }

    /// Search manifests by query string (matches name, display_name, description, keywords).
    pub fn search(&self, query: &str) -> Vec<&ExtensionManifest> {
        let query_lower = query.to_lowercase();
        let tokens: Vec<&str> = query_lower.split_whitespace().collect();

        let mut scored: Vec<(&ExtensionManifest, usize)> = self
            .manifests
            .values()
            .filter_map(|m| {
                let score = Self::score_manifest(m, &tokens);
                if score > 0 { Some((m, score)) } else { None }
            })
            .collect();

        scored.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.name.cmp(&b.0.name)));
        scored.into_iter().map(|(m, _)| m).collect()
    }

    fn score_manifest(manifest: &ExtensionManifest, tokens: &[&str]) -> usize {
        let mut score = 0;
        let name_lower = manifest.name.to_lowercase();
        let display_lower = manifest.display_name.to_lowercase();
        let desc_lower = manifest.description.to_lowercase();

        for token in tokens {
            if name_lower == *token {
                score += 10;
            } else if name_lower.contains(token) {
                score += 5;
            }

            if display_lower == *token {
                score += 8;
            } else if display_lower.contains(token) {
                score += 4;
            }

            if desc_lower.contains(token) {
                score += 2;
            }

            for kw in &manifest.keywords {
                if kw.to_lowercase() == *token {
                    score += 6;
                } else if kw.to_lowercase().contains(token) {
                    score += 3;
                }
            }

            for tag in &manifest.tags {
                if tag.to_lowercase() == *token {
                    score += 4;
                }
            }
        }

        score
    }

    /// Get a bundle definition by name.
    pub fn get_bundle(&self, name: &str) -> Option<&BundleDefinition> {
        self.bundles.get(name)
    }

    /// List all bundle names.
    pub fn bundle_names(&self) -> Vec<&str> {
        let mut names: Vec<_> = self.bundles.keys().map(|s| s.as_str()).collect();
        names.sort();
        names
    }

    /// Resolve a bundle into its constituent manifests.
    /// Returns the manifests and any extension keys that couldn't be found.
    pub fn resolve_bundle(
        &self,
        bundle_name: &str,
    ) -> Result<(Vec<&ExtensionManifest>, Vec<String>), RegistryError> {
        let bundle = self
            .bundles
            .get(bundle_name)
            .ok_or_else(|| RegistryError::BundleNotFound(bundle_name.to_string()))?;

        let mut found = Vec::new();
        let mut missing = Vec::new();

        for ext_key in &bundle.extensions {
            if let Some(manifest) = self.manifests.get(ext_key) {
                found.push(manifest);
            } else {
                missing.push(ext_key.clone());
            }
        }

        Ok((found, missing))
    }

    /// Check if a name refers to a bundle rather than an individual extension.
    pub fn is_bundle(&self, name: &str) -> bool {
        self.bundles.contains_key(name)
    }

    /// Resolve a name to either a single manifest or the manifests in a bundle.
    /// Returns (manifests, bundle_definition_if_bundle).
    pub fn resolve(
        &self,
        name: &str,
    ) -> Result<(Vec<&ExtensionManifest>, Option<&BundleDefinition>), RegistryError> {
        // Check bundle first
        if let Some(bundle) = self.bundles.get(name) {
            let (manifests, missing) = self.resolve_bundle(name)?;
            if !missing.is_empty() {
                tracing::warn!(
                    "Bundle '{}' references missing extensions: {:?}",
                    name,
                    missing
                );
            }
            return Ok((manifests, Some(bundle)));
        }

        // Single extension (use get_strict to catch ambiguous bare names)
        let manifest = self.get_strict(name)?;
        Ok((vec![manifest], None))
    }
}

#[cfg(test)]
mod tests;
