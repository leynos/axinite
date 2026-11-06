//! Extension, registry, pairing, skill, and auth-token DTOs for the
//! web gateway API.

use serde::{Deserialize, Serialize};

// --- Extensions ---

/// Summary of one extension as listed by the web gateway.
///
/// The `activation_*` fields are only populated for WASM channels.
#[derive(Debug, Serialize)]
pub struct ExtensionInfo {
    /// Unique extension name, used to address the extension in later requests.
    pub name: String,
    /// Human-readable label. Omitted when the extension does not provide one.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub display_name: Option<String>,
    /// Extension kind in snake_case: `mcp_server`, `wasm_tool`, `wasm_channel`
    /// or `channel_relay`.
    pub kind: String,
    /// Free-text summary of what the extension does.
    pub description: Option<String>,
    /// Endpoint or source URL for the extension. Omitted when unset.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    /// Whether the extension has the credentials it needs to run.
    pub authenticated: bool,
    /// Whether the extension is currently activated.
    pub active: bool,
    /// Names of the tools this extension contributes to the agent.
    pub tools: Vec<String>,
    /// Whether this extension has configurable secrets (setup schema).
    #[serde(default)]
    pub needs_setup: bool,
    /// Whether this extension has an auth configuration (OAuth or manual token).
    #[serde(default)]
    pub has_auth: bool,
    /// WASM channel activation status: "installed", "configured", "active", "failed".
    #[serde(skip_serializing_if = "Option::is_none")]
    pub activation_status: Option<String>,
    /// Human-readable error when activation_status is "failed".
    #[serde(skip_serializing_if = "Option::is_none")]
    pub activation_error: Option<String>,
    /// Extension version (semver).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
}

/// Response body listing the installed extensions.
#[derive(Debug, Serialize)]
pub struct ExtensionListResponse {
    /// Installed extensions visible to the gateway.
    pub extensions: Vec<ExtensionInfo>,
}

/// A tool available to the agent, as shown in the tools listing.
#[derive(Debug, Serialize)]
pub struct ToolInfo {
    /// Tool name as the LLM calls it.
    pub name: String,
    /// Description sent to the LLM alongside the tool's schema.
    pub description: String,
}

/// Response body listing the tools in the agent's registry.
#[derive(Debug, Serialize)]
pub struct ToolListResponse {
    /// Every tool definition currently registered.
    pub tools: Vec<ToolInfo>,
}

/// Request body installing an extension by name.
#[derive(Debug, Deserialize)]
pub struct InstallExtensionRequest {
    /// Registry name of the extension to install.
    pub name: String,
    /// Optional source URL passed to the installer alongside `name`.
    pub url: Option<String>,
    /// Optional kind hint: `mcp_server`, `wasm_tool` or `wasm_channel`.
    pub kind: Option<String>,
}

// --- Extension Setup ---

/// Setup schema for one extension: the secrets it asks the user to provide.
#[derive(Debug, Serialize)]
pub struct ExtensionSetupResponse {
    /// Extension name the schema belongs to.
    pub name: String,
    /// Extension kind in snake_case, or empty when the extension is not installed.
    pub kind: String,
    /// Secret fields the extension requires, in declaration order.
    pub secrets: Vec<SecretFieldInfo>,
}

/// One secret an extension asks the user to provide during setup.
#[derive(Debug, Serialize)]
pub struct SecretFieldInfo {
    /// Secret name, the key expected in the setup request.
    pub name: String,
    /// Prompt text shown to the user when requesting the value.
    pub prompt: String,
    /// Whether the secret may be left blank.
    pub optional: bool,
    /// Whether this secret is already stored.
    pub provided: bool,
    /// Whether the secret will be auto-generated if left empty.
    pub auto_generate: bool,
}

/// Request body submitting setup secrets for an extension.
#[derive(Debug, Deserialize)]
pub struct ExtensionSetupRequest {
    /// Secret values keyed by secret name. Names are checked against the
    /// extension's setup schema.
    pub secrets: std::collections::HashMap<String, String>,
}

/// Outcome of a state-changing extension or auth action.
#[derive(Debug, Serialize)]
pub struct ActionResponse {
    /// Whether the action succeeded.
    pub success: bool,
    /// Human-readable result, suitable for display to the user.
    pub message: String,
    /// Auth URL to open (when activation requires OAuth).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub auth_url: Option<String>,
    /// Whether the extension is waiting for a manual token.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub awaiting_token: Option<bool>,
    /// Instructions for manual token entry.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub instructions: Option<String>,
    /// Whether the channel was successfully activated after setup.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub activated: Option<bool>,
}

impl ActionResponse {
    /// Builds a successful response with only the message set.
    pub fn ok(message: impl Into<String>) -> Self {
        Self {
            success: true,
            message: message.into(),
            auth_url: None,
            awaiting_token: None,
            instructions: None,
            activated: None,
        }
    }

    /// Builds a failed response carrying the reason in `message`.
    pub fn fail(message: impl Into<String>) -> Self {
        Self {
            success: false,
            message: message.into(),
            auth_url: None,
            awaiting_token: None,
            instructions: None,
            activated: None,
        }
    }
}

// --- Registry ---

/// A registry entry offered in extension search results.
#[derive(Debug, Serialize)]
pub struct RegistryEntryInfo {
    /// Registry name, used to install the extension.
    pub name: String,
    /// Human-readable label for the entry.
    pub display_name: String,
    /// Extension kind in snake_case, as for `ExtensionInfo::kind`.
    pub kind: String,
    /// Short description of the entry.
    pub description: String,
    /// Search keywords the registry associates with the entry.
    pub keywords: Vec<String>,
    /// Whether an extension with the same name and kind is already installed.
    pub installed: bool,
    /// Version declared by the registry entry, if any.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
}

/// Response body for registry search.
#[derive(Debug, Serialize)]
pub struct RegistrySearchResponse {
    /// Entries matching the search query.
    pub entries: Vec<RegistryEntryInfo>,
}

/// Query parameters for registry search.
#[derive(Debug, Deserialize)]
pub struct RegistrySearchQuery {
    /// Whitespace-separated search terms, matched case-insensitively. Absent
    /// means no filter.
    pub query: Option<String>,
}

// --- Pairing ---

/// Pending pairing requests for one channel.
#[derive(Debug, Serialize)]
pub struct PairingListResponse {
    /// Channel name the requests belong to.
    pub channel: String,
    /// Requests awaiting approval.
    pub requests: Vec<PairingRequestInfo>,
}

/// A pending request from an unknown sender to pair with a channel.
#[derive(Debug, Serialize)]
pub struct PairingRequestInfo {
    /// One-time code the user enters to approve the pairing.
    pub code: String,
    /// Channel-specific identifier of the sender being paired.
    pub sender_id: String,
    /// Channel-supplied metadata as arbitrary JSON. Omitted when absent.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub meta: Option<serde_json::Value>,
    /// RFC 3339 timestamp of when the request was first created.
    pub created_at: String,
}

/// Request body approving a pending pairing request.
#[derive(Debug, Deserialize)]
pub struct PairingApproveRequest {
    /// Pairing code issued for the pending request.
    pub code: String,
}

// --- Skills ---

/// Summary of a skill, either installed or matched by a search.
#[derive(Debug, Serialize)]
pub struct SkillInfo {
    /// Skill name from its manifest.
    pub name: String,
    /// Skill description from its manifest.
    pub description: String,
    /// Skill version from its manifest.
    pub version: String,
    /// Trust level: `trusted` for user-placed skills, `installed` for registry skills.
    pub trust: String,
    /// Debug rendering of where the skill was loaded from.
    pub source: String,
    /// Activation keywords that select the skill for a message.
    pub keywords: Vec<String>,
}

/// Response body listing the loaded skills.
#[derive(Debug, Serialize)]
pub struct SkillListResponse {
    /// Loaded skills.
    pub skills: Vec<SkillInfo>,
    /// Number of entries in `skills`.
    pub count: usize,
}

/// Request body searching the skill catalogue and the installed skills.
#[derive(Debug, Deserialize)]
pub struct SkillSearchRequest {
    /// Search term. Installed skills are matched case-insensitively against
    /// name and description.
    pub query: String,
}

/// Results from the remote skill catalogue and the locally installed skills.
#[derive(Debug, Serialize)]
pub struct SkillSearchResponse {
    /// Catalogue hits as JSON objects with `slug`, `name`, `description`,
    /// `version` and `score` fields.
    pub catalog: Vec<serde_json::Value>,
    /// Locally installed skills matching the query.
    pub installed: Vec<SkillInfo>,
    /// Base URL of the catalogue that was searched.
    pub registry_url: String,
    /// If the catalogue registry was unreachable or errored, a human-readable message.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub catalog_error: Option<String>,
}

/// Request body installing a skill. Exactly one source must be supplied:
/// `content`, `url`, or a catalogue key (`slug`, else `name`).
#[derive(Debug, Deserialize)]
pub struct SkillInstallRequest {
    /// Catalogue name of the skill. Used only when `slug` is absent.
    pub name: Option<String>,
    /// Registry slug (e.g. "owner/skill-name"). Preferred over `name` for
    /// constructing the download URL when fetching from ClawHub.
    pub slug: Option<String>,
    /// Direct URL to fetch the skill bundle from.
    pub url: Option<String>,
    /// Inline SKILL.md content to install directly.
    pub content: Option<String>,
}

// --- Auth Token ---

/// Request to submit an auth token for an extension (dedicated endpoint).
#[derive(Debug, Deserialize)]
pub struct AuthTokenRequest {
    /// Extension whose pending auth flow receives the token.
    pub extension_name: String,
    /// Token value supplied by the user for a manual auth step.
    pub token: String,
}

/// Request to cancel an in-progress auth flow.
#[derive(Debug, Deserialize)]
pub struct AuthCancelRequest {
    /// Extension whose in-progress auth flow is cancelled.
    pub extension_name: String,
}
