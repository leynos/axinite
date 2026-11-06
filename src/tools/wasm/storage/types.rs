//! Domain types for stored WASM tools: trust levels, statuses, stored
//! records, capabilities, parameters, and storage errors.

use std::collections::HashMap;

use chrono::{DateTime, Utc};
use uuid::Uuid;

use crate::tools::wasm::capabilities::{
    Capabilities, EndpointPattern, HttpCapability, RateLimitConfig, SecretsCapability,
    ToolInvokeCapability,
};

/// Trust level for a WASM tool.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrustLevel {
    /// Built-in system tool (highest trust).
    System,
    /// Audited and verified tool.
    Verified,
    /// User-uploaded tool (untrusted).
    User,
}

impl std::fmt::Display for TrustLevel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TrustLevel::System => write!(f, "system"),
            TrustLevel::Verified => write!(f, "verified"),
            TrustLevel::User => write!(f, "user"),
        }
    }
}

impl std::str::FromStr for TrustLevel {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "system" => Ok(TrustLevel::System),
            "verified" => Ok(TrustLevel::Verified),
            "user" => Ok(TrustLevel::User),
            _ => Err(format!("Unknown trust level: {}", s)),
        }
    }
}

/// Status of a WASM tool.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolStatus {
    /// Tool is active and can be used.
    Active,
    /// Tool is disabled (manually or due to errors).
    Disabled,
    /// Tool is quarantined (suspected malicious).
    Quarantined,
}

impl std::fmt::Display for ToolStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ToolStatus::Active => write!(f, "active"),
            ToolStatus::Disabled => write!(f, "disabled"),
            ToolStatus::Quarantined => write!(f, "quarantined"),
        }
    }
}

impl std::str::FromStr for ToolStatus {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "active" => Ok(ToolStatus::Active),
            "disabled" => Ok(ToolStatus::Disabled),
            "quarantined" => Ok(ToolStatus::Quarantined),
            _ => Err(format!("Unknown status: {}", s)),
        }
    }
}

/// A stored WASM tool.
#[derive(Debug, Clone)]
pub struct StoredWasmTool {
    /// Primary key of the `wasm_tools` row.
    pub id: Uuid,
    /// Owner of the tool. Tools are scoped per user, so the same name may exist for
    /// several owners.
    pub user_id: String,
    /// Tool name. Rows are unique per owner, name and version.
    pub name: String,
    /// Tool version string as declared by the extension.
    pub version: String,
    /// WIT interface version the component targets, such as `0.3.0`.
    pub wit_version: String,
    /// Human-readable description of what the tool does.
    pub description: String,
    /// JSON Schema describing the tool's parameters.
    pub parameters_schema: serde_json::Value,
    /// Where the tool was obtained from, if it was fetched from a URL.
    pub source_url: Option<String>,
    /// Trust level recorded when the tool was stored (`system`, `verified` or `user`).
    pub trust_level: TrustLevel,
    /// Lifecycle state. `get` refuses tools that are not `Active`.
    pub status: ToolStatus,
    /// When the tool was first stored.
    pub created_at: DateTime<Utc>,
    /// When the tool row was last changed.
    pub updated_at: DateTime<Utc>,
}

/// Full tool data including binary (not returned by default for efficiency).
#[derive(Debug)]
pub struct StoredWasmToolWithBinary {
    /// Tool metadata, as in `StoredWasmTool`.
    pub tool: StoredWasmTool,
    /// Raw WASM component bytes.
    pub wasm_binary: Vec<u8>,
    /// BLAKE3 digest of `wasm_binary` (32 bytes), checked before the binary is used.
    pub binary_hash: Vec<u8>,
}

/// Capabilities stored in the database.
#[derive(Debug, Clone)]
pub struct StoredCapabilities {
    /// Primary key of the capabilities row.
    pub id: Uuid,
    /// The tool these capabilities belong to.
    pub wasm_tool_id: Uuid,
    /// Endpoint patterns the tool may call over HTTP. Empty means no HTTP access.
    pub http_allowlist: Vec<EndpointPattern>,
    /// Names of secrets the tool may read. Only names are stored, never values.
    pub allowed_secrets: Vec<String>,
    /// Map from the alias a tool calls by to the real tool name it resolves to.
    pub tool_aliases: HashMap<String, String>,
    /// Per-minute HTTP and tool-invocation budget.
    pub requests_per_minute: u32,
    /// Per-hour HTTP and tool-invocation budget.
    pub requests_per_hour: u32,
    /// Largest HTTP request body the tool may send, in bytes.
    pub max_request_body_bytes: i64,
    /// Largest HTTP response body the tool may receive, in bytes.
    pub max_response_body_bytes: i64,
    /// Workspace path prefixes the tool may read from. Empty means no workspace read.
    pub workspace_read_prefixes: Vec<String>,
    /// Timeout for each HTTP request, in whole seconds.
    pub http_timeout_secs: i32,
}

impl StoredCapabilities {
    /// Convert to runtime Capabilities struct.
    pub fn to_capabilities(&self) -> Capabilities {
        let mut caps = Capabilities::default();

        // Workspace read
        if !self.workspace_read_prefixes.is_empty() {
            caps = caps.with_workspace_read(self.workspace_read_prefixes.clone());
        }

        // HTTP capability
        if !self.http_allowlist.is_empty() {
            caps.http = Some(HttpCapability {
                allowlist: self.http_allowlist.clone(),
                credentials: HashMap::new(), // Loaded separately
                rate_limit: RateLimitConfig {
                    requests_per_minute: self.requests_per_minute,
                    requests_per_hour: self.requests_per_hour,
                },
                max_request_bytes: self.max_request_body_bytes as usize,
                max_response_bytes: self.max_response_body_bytes as usize,
                timeout: std::time::Duration::from_secs(self.http_timeout_secs as u64),
            });
        }

        // Tool invoke capability
        if !self.tool_aliases.is_empty() {
            caps.tool_invoke = Some(ToolInvokeCapability {
                aliases: self.tool_aliases.clone(),
                rate_limit: RateLimitConfig {
                    requests_per_minute: self.requests_per_minute,
                    requests_per_hour: self.requests_per_hour,
                },
            });
        }

        // Secrets capability
        if !self.allowed_secrets.is_empty() {
            caps.secrets = Some(SecretsCapability {
                allowed_names: self.allowed_secrets.clone(),
            });
        }

        caps
    }
}

/// Error from WASM storage operations.
#[derive(Debug, Clone, thiserror::Error)]
pub enum WasmStorageError {
    /// No tool with this name exists for the owner. Carries the tool name.
    #[error("Tool not found: {0}")]
    NotFound(String),

    /// The tool exists but its status is `ToolStatus::Disabled`.
    #[error("Tool is disabled")]
    Disabled,

    /// The tool exists but its status is `ToolStatus::Quarantined`.
    #[error("Tool is quarantined")]
    Quarantined,

    /// The stored binary no longer matches its recorded `binary_hash`, so it is not used.
    #[error("Binary integrity check failed: hash mismatch")]
    IntegrityCheckFailed,

    /// A failure reported by the database driver. Carries the driver's message.
    #[error("Database error: {0}")]
    Database(String),

    /// A stored row could not be decoded into a domain value, such as an unparsable
    /// UUID or an unknown status string. Carries the decoding error message.
    #[error("Invalid data: {0}")]
    InvalidData(String),
}

/// Identifies a stored WASM tool by owner and name.
#[derive(Debug, Clone, Copy)]
pub struct ToolKey<'a> {
    /// Owner whose tool is addressed. Lookups never cross owners.
    pub user_id: &'a str,
    /// Name of the tool under that owner.
    pub name: &'a str,
}

/// Parameters for storing a new tool.
pub struct StoreToolParams {
    /// Owner the tool is stored under.
    pub user_id: String,
    /// Tool name, unique per owner and version.
    pub name: String,
    /// Tool version string as declared by the extension.
    pub version: String,
    /// WIT interface version the component targets.
    pub wit_version: String,
    /// Human-readable description of what the tool does.
    pub description: String,
    /// Raw WASM component bytes. The storage layer computes and records their hash.
    pub wasm_binary: Vec<u8>,
    /// JSON Schema describing the tool's parameters.
    pub parameters_schema: serde_json::Value,
    /// Where the tool was obtained from, if it was fetched from a URL.
    pub source_url: Option<String>,
    /// Trust level to record for the tool.
    pub trust_level: TrustLevel,
}
