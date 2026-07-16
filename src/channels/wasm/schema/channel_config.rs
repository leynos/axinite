//! Runtime channel configuration schema returned by `on_start`.
//!
//! Describes the HTTP endpoints and polling configuration a WASM channel
//! registers when it starts.

use serde::{Deserialize, Serialize};

/// Channel configuration returned by on_start.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChannelConfig {
    /// Display name for the channel.
    pub display_name: String,

    /// HTTP endpoints to register.
    #[serde(default)]
    pub http_endpoints: Vec<HttpEndpointConfigSchema>,

    /// Polling configuration.
    #[serde(default)]
    pub poll: Option<PollConfigSchema>,
}

impl Default for ChannelConfig {
    fn default() -> Self {
        Self {
            display_name: "WASM Channel".to_string(),
            http_endpoints: Vec::new(),
            poll: None,
        }
    }
}

/// HTTP endpoint configuration schema.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HttpEndpointConfigSchema {
    /// Path to register.
    pub path: String,

    /// HTTP methods to accept.
    #[serde(default)]
    pub methods: Vec<String>,

    /// Whether secret validation is required.
    #[serde(default)]
    pub require_secret: bool,
}

/// Polling configuration schema.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PollConfigSchema {
    /// Polling interval in milliseconds.
    pub interval_ms: u32,

    /// Whether polling is enabled.
    #[serde(default)]
    pub enabled: bool,
}
