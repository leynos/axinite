//! Descriptive metadata for extensions: kinds, registry entries, sources,
//! and authentication hints.

use serde::{Deserialize, Serialize};

/// The kind of extension, determining how it's installed, authenticated, and activated.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExtensionKind {
    /// Hosted MCP server, HTTP transport, OAuth 2.1 auth.
    McpServer,
    /// Sandboxed WASM module, file-based, capabilities auth.
    WasmTool,
    /// WASM channel module with hot-activation support.
    WasmChannel,
    /// External channel via channel-relay service (Slack, etc.).
    ChannelRelay,
}

impl std::fmt::Display for ExtensionKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ExtensionKind::McpServer => write!(f, "mcp_server"),
            ExtensionKind::WasmTool => write!(f, "wasm_tool"),
            ExtensionKind::WasmChannel => write!(f, "wasm_channel"),
            ExtensionKind::ChannelRelay => write!(f, "channel_relay"),
        }
    }
}

/// A registry entry describing a known or discovered extension.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegistryEntry {
    /// Unique identifier (e.g., "notion", "weather", "telegram").
    pub name: String,
    /// Human-readable name (e.g., "Notion", "Weather Tool").
    pub display_name: String,
    /// What kind of extension this is.
    pub kind: ExtensionKind,
    /// Short description of what this extension does.
    pub description: String,
    /// Search keywords beyond the name.
    #[serde(default)]
    pub keywords: Vec<String>,
    /// Where to get this extension.
    pub source: ExtensionSource,
    /// Fallback source when the primary source fails (e.g., download 404 → build from source).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fallback_source: Option<Box<ExtensionSource>>,
    /// How authentication works.
    pub auth_hint: AuthHint,
    /// Extension version (semver), if known.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
}

/// Where the extension binary or server lives.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ExtensionSource {
    /// URL to a hosted MCP server.
    McpUrl { url: String },
    /// Downloadable WASM binary.
    WasmDownload {
        wasm_url: String,
        #[serde(default)]
        capabilities_url: Option<String>,
    },
    /// Build from local source directory.
    WasmBuildable {
        #[serde(alias = "repo_url")]
        source_dir: String,
        #[serde(default)]
        build_dir: Option<String>,
        /// Crate name used to locate the build artifact binary.
        #[serde(default)]
        crate_name: Option<String>,
    },
    /// Discovered online (not yet validated for a specific source type).
    Discovered { url: String },
    /// External channel via channel-relay service.
    ChannelRelay { relay_url: String },
}

/// Hint about what authentication method is needed.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum AuthHint {
    /// MCP server supports Dynamic Client Registration (zero-config OAuth).
    Dcr,
    /// MCP server needs a pre-configured OAuth client_id.
    OAuthPreConfigured {
        /// URL where the user can create an OAuth app.
        setup_url: String,
    },
    /// WASM tool has auth defined in its capabilities.json file.
    CapabilitiesAuth,
    /// No authentication needed.
    None,
    /// OAuth via channel-relay service.
    ChannelRelayOAuth,
}

/// Where a search result came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ResultSource {
    /// From the built-in curated registry.
    Registry,
    /// From online discovery (validated).
    Discovered,
}
