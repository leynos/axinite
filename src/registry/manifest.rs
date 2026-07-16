//! Serde structs for extension registry manifests.
//!
//! Each manifest describes a single extension (tool or channel) with its source
//! location, build artifacts, authentication requirements, and tags.

use serde::{Deserialize, Serialize};

use crate::extensions::{AuthHint, ExtensionKind, ExtensionSource, RegistryEntry};

/// A single extension manifest loaded from `registry/{tools,channels}/<name>.json`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExtensionManifest {
    /// Unique identifier (matches crate name stem, e.g. "slack").
    pub name: String,

    /// Human-readable name (e.g. "Slack").
    pub display_name: String,

    /// Whether this is a tool or channel.
    pub kind: ManifestKind,

    /// Semver version from Cargo.toml.
    pub version: String,

    /// One-line description.
    pub description: String,

    /// Search keywords beyond the name.
    #[serde(default)]
    pub keywords: Vec<String>,

    /// Source code location and build info.
    pub source: SourceSpec,

    /// Pre-built binary artifacts keyed by target triple.
    #[serde(default)]
    pub artifacts: std::collections::HashMap<String, ArtifactSpec>,

    /// Summary of authentication requirements.
    #[serde(default)]
    pub auth_summary: Option<AuthSummary>,

    /// Tags for filtering (e.g. "default", "messaging", "google").
    #[serde(default)]
    pub tags: Vec<String>,
}

/// Extension kind as declared in manifests.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ManifestKind {
    Tool,
    Channel,
}

impl From<ManifestKind> for ExtensionKind {
    fn from(kind: ManifestKind) -> Self {
        match kind {
            ManifestKind::Tool => ExtensionKind::WasmTool,
            ManifestKind::Channel => ExtensionKind::WasmChannel,
        }
    }
}

impl std::fmt::Display for ManifestKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ManifestKind::Tool => write!(f, "tool"),
            ManifestKind::Channel => write!(f, "channel"),
        }
    }
}

/// Source code location for building from source.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SourceSpec {
    /// Path relative to repo root (e.g. "tools-src/slack").
    pub dir: String,

    /// Capabilities filename relative to source dir.
    pub capabilities: String,

    /// Rust crate name for `cargo component build`.
    pub crate_name: String,
}

/// A pre-built binary artifact.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArtifactSpec {
    /// Download URL (null until release).
    /// Can point to a `.wasm` file or a `.tar.gz` bundle containing both
    /// `{name}.wasm` and `{name}.capabilities.json`.
    pub url: Option<String>,

    /// Hex SHA256 of the downloaded artifact (null until release).
    pub sha256: Option<String>,

    /// Optional separate download URL for the capabilities file.
    /// Only needed when `url` points to a bare `.wasm` file instead of a bundle.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub capabilities_url: Option<String>,
}

/// Summary of authentication requirements extracted from capabilities.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuthSummary {
    /// Auth method: "oauth", "manual", or "none".
    #[serde(default)]
    pub method: Option<String>,

    /// Display name for the auth provider (e.g. "Google", "Slack").
    #[serde(default)]
    pub provider: Option<String>,

    /// Secret names required by this extension.
    #[serde(default)]
    pub secrets: Vec<String>,

    /// If this extension shares auth with others (e.g. all Google tools share
    /// `google_oauth_token`), this is the shared secret name.
    #[serde(default)]
    pub shared_auth: Option<String>,

    /// URL where users can set up credentials.
    #[serde(default)]
    pub setup_url: Option<String>,
}

/// Bundle definition grouping related extensions.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BundleDefinition {
    /// Human-readable name.
    pub display_name: String,

    /// Description of what this bundle contains.
    #[serde(default)]
    pub description: Option<String>,

    /// Extension references as "tools/<name>" or "channels/<name>".
    pub extensions: Vec<String>,

    /// Shared auth secret across bundle members (if any).
    #[serde(default)]
    pub shared_auth: Option<String>,
}

/// Top-level structure of `_bundles.json`.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct BundlesFile {
    pub bundles: std::collections::HashMap<String, BundleDefinition>,
}

impl ExtensionManifest {
    /// Convert this manifest into a [`RegistryEntry`] for use with the in-chat
    /// extension discovery system.
    pub fn to_registry_entry(&self) -> RegistryEntry {
        let buildable = ExtensionSource::WasmBuildable {
            source_dir: self.source.dir.clone(),
            build_dir: Some(self.source.dir.clone()),
            crate_name: Some(self.source.crate_name.clone()),
        };

        // Prefer pre-built artifact download when a URL is available,
        // with build-from-source as fallback in case the download fails (e.g., 404).
        let (source, fallback_source) = if let Some(artifact) = self.artifacts.get("wasm32-wasip2")
        {
            if let Some(ref url) = artifact.url {
                (
                    ExtensionSource::WasmDownload {
                        wasm_url: url.clone(),
                        capabilities_url: artifact.capabilities_url.clone(),
                    },
                    Some(Box::new(buildable)),
                )
            } else {
                (buildable, None)
            }
        } else {
            (buildable, None)
        };

        let auth_hint = match self.auth_summary.as_ref().and_then(|a| a.method.as_deref()) {
            Some("oauth") => AuthHint::CapabilitiesAuth,
            Some("manual") => AuthHint::CapabilitiesAuth,
            Some("none") | None => AuthHint::None,
            Some(_) => AuthHint::CapabilitiesAuth,
        };

        RegistryEntry {
            name: self.name.clone(),
            display_name: self.display_name.clone(),
            kind: self.kind.into(),
            description: self.description.clone(),
            keywords: self.keywords.clone(),
            source,
            fallback_source,
            auth_hint,
            version: Some(self.version.clone()),
        }
    }
}

#[cfg(test)]
mod tests;
