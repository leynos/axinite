//! Operation results for extension lifecycle actions (search, install,
//! upgrade, activate), installed-extension listings, and the error type.

use serde::{Deserialize, Serialize};

use super::descriptor::{ExtensionKind, RegistryEntry, ResultSource};

/// Result of searching for extensions.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchResult {
    /// The registry entry.
    #[serde(flatten)]
    pub entry: RegistryEntry,
    /// Where this result came from.
    pub source: ResultSource,
    /// Whether the endpoint was validated (for discovered entries).
    #[serde(default)]
    pub validated: bool,
}

/// Result of installing an extension.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InstallResult {
    pub name: String,
    pub kind: ExtensionKind,
    pub message: String,
}

/// Result of upgrading one or more extensions.
#[derive(Debug, Clone, serde::Serialize)]
pub struct UpgradeResult {
    /// Per-extension upgrade outcomes.
    pub results: Vec<UpgradeOutcome>,
    /// Summary message.
    pub message: String,
}

/// Outcome for a single extension upgrade.
#[derive(Debug, Clone, serde::Serialize)]
pub struct UpgradeOutcome {
    pub name: String,
    pub kind: ExtensionKind,
    /// What happened: "upgraded", "already_up_to_date", "failed", "not_in_registry".
    pub status: String,
    /// Human-readable detail.
    pub detail: String,
}

/// Result of activating an extension.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActivateResult {
    pub name: String,
    pub kind: ExtensionKind,
    /// Names of tools that were loaded/registered.
    pub tools_loaded: Vec<String>,
    pub message: String,
}

fn default_true() -> bool {
    true
}

/// An installed extension with its current status.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InstalledExtension {
    pub name: String,
    pub kind: ExtensionKind,
    /// Human-readable display name (e.g. "Telegram Channel" vs "Telegram Tool").
    #[serde(skip_serializing_if = "Option::is_none")]
    pub display_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// Server or source URL (e.g. MCP server endpoint).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    pub authenticated: bool,
    pub active: bool,
    /// Tool names if active.
    #[serde(default)]
    pub tools: Vec<String>,
    /// Whether this extension has a setup schema (required_secrets) that can be configured.
    #[serde(default)]
    pub needs_setup: bool,
    /// Whether this extension has an auth configuration (OAuth or manual token).
    #[serde(default)]
    pub has_auth: bool,
    /// Whether this extension is installed locally (false = available in registry but not installed).
    #[serde(default = "default_true")]
    pub installed: bool,
    /// Last activation error for WASM channels.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub activation_error: Option<String>,
    /// Extension version from capabilities file (semver).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
}

/// Error type for extension operations.
#[derive(Debug, thiserror::Error)]
pub enum ExtensionError {
    #[error("Extension not found: {0}")]
    NotFound(String),

    #[error("Extension already installed: {0}")]
    AlreadyInstalled(String),

    #[error("Extension not installed: {0}")]
    NotInstalled(String),

    #[error("Authentication failed: {0}")]
    AuthFailed(String),

    #[error("Activation failed: {0}")]
    ActivationFailed(String),

    #[error("Authentication required")]
    AuthRequired,

    #[error("Installation failed: {0}")]
    InstallFailed(String),

    #[error("Discovery failed: {0}")]
    DiscoveryFailed(String),

    #[error("Invalid URL: {0}")]
    InvalidUrl(String),

    #[error("Download failed: {0}")]
    DownloadFailed(String),

    #[error("Config error: {0}")]
    Config(String),

    #[error("Primary install failed: {primary}; fallback install also failed: {fallback}")]
    FallbackFailed {
        primary: Box<ExtensionError>,
        fallback: Box<ExtensionError>,
    },

    #[error("{0}")]
    Other(String),
}
