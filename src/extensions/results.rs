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
    /// Name of the extension that was installed.
    pub name: String,
    /// Whether the extension is a tool or a channel.
    pub kind: ExtensionKind,
    /// Human-readable summary of the outcome.
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
    /// Name of the extension the upgrade was attempted for.
    pub name: String,
    /// Whether the extension is a tool or a channel.
    pub kind: ExtensionKind,
    /// What happened: "upgraded", "already_up_to_date", "failed", "not_in_registry".
    pub status: String,
    /// Human-readable detail.
    pub detail: String,
}

/// Result of activating an extension.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActivateResult {
    /// Name of the extension that was activated.
    pub name: String,
    /// Whether the extension is a tool or a channel.
    pub kind: ExtensionKind,
    /// Names of tools that were loaded/registered.
    pub tools_loaded: Vec<String>,
    /// Human-readable summary of the activation.
    pub message: String,
}

fn default_true() -> bool {
    true
}

/// An installed extension with its current status.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InstalledExtension {
    /// Name of the extension, as used in the registry and tool arguments.
    pub name: String,
    /// Whether the extension is a tool, a channel or an MCP server.
    pub kind: ExtensionKind,
    /// Human-readable display name (e.g. "Telegram Channel" vs "Telegram Tool").
    #[serde(skip_serializing_if = "Option::is_none")]
    pub display_name: Option<String>,
    /// Free-text summary of what the extension does, omitted from JSON when unset.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// Server or source URL (e.g. MCP server endpoint).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    /// Whether the stored credentials for this extension are present.
    pub authenticated: bool,
    /// Whether the extension is currently running with its tools loaded.
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
    /// No extension matches the requested name.
    #[error("Extension not found: {0}")]
    NotFound(String),

    /// The extension is already installed. Install fallback is not attempted for this case.
    #[error("Extension already installed: {0}")]
    AlreadyInstalled(String),

    /// The operation needs an installed extension, but none with that name exists.
    #[error("Extension not installed: {0}")]
    NotInstalled(String),

    /// Authenticating with the extension's provider failed. The string gives the cause.
    #[error("Authentication failed: {0}")]
    AuthFailed(String),

    /// The extension is installed but could not be activated. The string gives the cause.
    #[error("Activation failed: {0}")]
    ActivationFailed(String),

    /// Activation needs credentials that have not been supplied yet.
    #[error("Authentication required")]
    AuthRequired,

    /// Installing the extension failed. The string gives the cause.
    #[error("Installation failed: {0}")]
    InstallFailed(String),

    /// Searching for extensions online failed.
    #[error("Discovery failed: {0}")]
    DiscoveryFailed(String),

    /// The URL supplied for an install or lookup is not valid.
    #[error("Invalid URL: {0}")]
    InvalidUrl(String),

    /// Fetching an extension artifact over the network failed.
    #[error("Download failed: {0}")]
    DownloadFailed(String),

    /// Extension configuration could not be read or written.
    #[error("Config error: {0}")]
    Config(String),

    /// Both the primary install and the fallback install path failed.
    #[error("Primary install failed: {primary}; fallback install also failed: {fallback}")]
    FallbackFailed {
        /// Error from the primary install path.
        primary: Box<ExtensionError>,
        /// Error from the fallback install path.
        fallback: Box<ExtensionError>,
    },

    /// Any other failure. The string is the message.
    #[error("{0}")]
    Other(String),
}
