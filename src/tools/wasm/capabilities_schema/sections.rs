//! Simple capability sections: secret existence checks, tool invocation
//! aliases, and workspace read access.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use super::http::RateLimitSchema;

/// Secrets capability schema.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SecretsCapabilitySchema {
    /// Secret names the tool can check existence of (supports glob).
    #[serde(default)]
    pub allowed_names: Vec<String>,
}

/// Tool invocation capability schema.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ToolInvokeCapabilitySchema {
    /// Mapping from alias to real tool name.
    #[serde(default)]
    pub aliases: HashMap<String, String>,

    /// Rate limiting for tool calls.
    #[serde(default)]
    pub rate_limit: Option<RateLimitSchema>,
}

/// Workspace read capability schema.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct WorkspaceCapabilitySchema {
    /// Allowed path prefixes (e.g., ["context/", "daily/"]).
    #[serde(default)]
    pub allowed_prefixes: Vec<String>,
}
