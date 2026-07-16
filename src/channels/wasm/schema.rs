//! JSON schema for WASM channel capabilities files.
//!
//! External WASM channels declare their required capabilities via a sidecar JSON file
//! (e.g., `slack.capabilities.json`). This module defines the schema for those files
//! and provides conversion to runtime [`ChannelCapabilities`].
//!
//! # Example Capabilities File
//!
//! ```json
//! {
//!   "type": "channel",
//!   "name": "slack",
//!   "description": "Slack Events API channel",
//!   "capabilities": {
//!     "http": {
//!       "allowlist": [
//!         { "host": "slack.com", "path_prefix": "/api/" }
//!       ],
//!       "credentials": {
//!         "slack_bot": {
//!           "secret_name": "slack_bot_token",
//!           "location": { "type": "bearer" },
//!           "host_patterns": ["slack.com"]
//!         }
//!       }
//!     },
//!     "secrets": { "allowed_names": ["slack_*"] },
//!     "channel": {
//!       "allowed_paths": ["/webhook/slack"],
//!       "allow_polling": false,
//!       "workspace_prefix": "channels/slack/",
//!       "emit_rate_limit": { "messages_per_minute": 100 }
//!     }
//!   },
//!   "config": {
//!     "signing_secret_name": "slack_signing_secret"
//!   }
//! }
//! ```

use std::collections::HashMap;
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::channels::wasm::capabilities::{
    ChannelCapabilities, EmitRateLimitConfig, MIN_POLL_INTERVAL_MS,
};
use crate::tools::wasm::{CapabilitiesFile as ToolCapabilitiesFile, RateLimitSchema};

mod channel_config;
mod setup;

pub use channel_config::{ChannelConfig, HttpEndpointConfigSchema, PollConfigSchema};
pub use setup::{SecretSetupSchema, SetupSchema};

/// Root schema for a channel capabilities JSON file.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ChannelCapabilitiesFile {
    /// Extension version (semver).
    #[serde(default)]
    pub version: Option<String>,

    /// WIT interface version this channel was compiled against (semver).
    #[serde(default)]
    pub wit_version: Option<String>,

    /// File type, must be "channel".
    #[serde(default = "default_type")]
    pub r#type: String,

    /// Channel name.
    pub name: String,

    /// Channel description.
    #[serde(default)]
    pub description: Option<String>,

    /// Setup configuration for the wizard.
    #[serde(default)]
    pub setup: SetupSchema,

    /// Capabilities (tool + channel specific).
    #[serde(default)]
    pub capabilities: ChannelCapabilitiesSchema,

    /// Channel-specific configuration passed to on_start.
    #[serde(default)]
    pub config: HashMap<String, serde_json::Value>,
}

fn default_type() -> String {
    "channel".to_string()
}

impl ChannelCapabilitiesFile {
    /// Parse from JSON string.
    pub fn from_json(json: &str) -> Result<Self, serde_json::Error> {
        serde_json::from_str(json)
    }

    /// Parse from JSON bytes.
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, serde_json::Error> {
        serde_json::from_slice(bytes)
    }

    /// Validate the capabilities file and emit warnings for common misconfigurations.
    ///
    /// Called once at load time to catch issues early. Warnings are emitted via
    /// `tracing::warn` so they show up in startup logs without blocking loading.
    pub fn validate(&self) {
        const MIN_PROMPT_LENGTH: usize = 30;

        // Check for short prompts in required_secrets
        for secret in &self.setup.required_secrets {
            if secret.prompt.len() < MIN_PROMPT_LENGTH {
                tracing::warn!(
                    channel = self.name,
                    secret = secret.name,
                    prompt = secret.prompt,
                    "setup.required_secrets prompt is shorter than {} chars — \
                     consider a more descriptive prompt that tells the user where to find this value",
                    MIN_PROMPT_LENGTH
                );
            }
        }

        // Has required_secrets but no setup_url
        if !self.setup.required_secrets.is_empty() && self.setup.setup_url.is_none() {
            tracing::warn!(
                channel = self.name,
                "setup.required_secrets defined but no setup.setup_url — \
                 user has no link to obtain credentials"
            );
        }
    }

    /// Convert to runtime ChannelCapabilities.
    pub fn to_capabilities(&self) -> ChannelCapabilities {
        self.capabilities.to_channel_capabilities(&self.name)
    }

    /// Get the channel config as JSON string.
    pub fn config_json(&self) -> String {
        serde_json::to_string(&self.config).unwrap_or_else(|_| "{}".to_string())
    }

    /// Get the webhook secret header name for this channel.
    ///
    /// Returns the configured header name from capabilities, or a sensible default.
    pub fn webhook_secret_header(&self) -> Option<&str> {
        self.capabilities
            .channel
            .as_ref()
            .and_then(|c| c.webhook.as_ref())
            .and_then(|w| w.secret_header.as_deref())
    }

    /// Get the signature verification key secret name for this channel.
    ///
    /// Returns the secret name declared in `webhook.signature_key_secret_name`,
    /// used to look up the Ed25519 public key in the secrets store.
    pub fn signature_key_secret_name(&self) -> Option<&str> {
        self.capabilities
            .channel
            .as_ref()
            .and_then(|c| c.webhook.as_ref())
            .and_then(|w| w.signature_key_secret_name.as_deref())
    }

    /// Get the HMAC-SHA256 signing secret name for this channel.
    ///
    /// Returns the secret name declared in `webhook.hmac_secret_name`,
    /// used to look up the HMAC signing secret in the secrets store (Slack-style).
    pub fn hmac_secret_name(&self) -> Option<&str> {
        self.capabilities
            .channel
            .as_ref()
            .and_then(|c| c.webhook.as_ref())
            .and_then(|w| w.hmac_secret_name.as_deref())
    }

    /// Get the webhook secret name for this channel.
    ///
    /// Returns the configured secret name or defaults to "{channel_name}_webhook_secret".
    pub fn webhook_secret_name(&self) -> String {
        self.capabilities
            .channel
            .as_ref()
            .and_then(|c| c.webhook.as_ref())
            .and_then(|w| w.secret_name.clone())
            .unwrap_or_else(|| format!("{}_webhook_secret", self.name))
    }
}

/// Schema for channel capabilities.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ChannelCapabilitiesSchema {
    /// Tool capabilities (HTTP, secrets, workspace_read).
    /// Note: Using the struct directly (not Option) because #[serde(flatten)]
    /// with Option<T> doesn't work correctly when T has all-optional fields.
    #[serde(flatten)]
    pub tool: ToolCapabilitiesFile,

    /// Channel-specific capabilities.
    #[serde(default)]
    pub channel: Option<ChannelSpecificCapabilitiesSchema>,
}

impl ChannelCapabilitiesSchema {
    /// Convert to runtime ChannelCapabilities.
    pub fn to_channel_capabilities(&self, channel_name: &str) -> ChannelCapabilities {
        let tool_caps = self.tool.to_capabilities();

        let mut caps =
            ChannelCapabilities::for_channel(channel_name).with_tool_capabilities(tool_caps);

        if let Some(channel) = &self.channel {
            caps.allowed_paths = channel.allowed_paths.clone();
            caps.allow_polling = channel.allow_polling;
            caps.min_poll_interval_ms = channel
                .min_poll_interval_ms
                .unwrap_or(MIN_POLL_INTERVAL_MS)
                .max(MIN_POLL_INTERVAL_MS);

            if let Some(prefix) = &channel.workspace_prefix {
                caps.workspace_prefix = prefix.clone();
            }

            if let Some(rate) = &channel.emit_rate_limit {
                caps.emit_rate_limit = rate.to_emit_rate_limit();
            }

            if let Some(max_size) = channel.max_message_size {
                caps.max_message_size = max_size;
            }

            if let Some(timeout_secs) = channel.callback_timeout_secs {
                caps.callback_timeout = Duration::from_secs(timeout_secs);
            }
        }

        caps
    }
}

/// Channel-specific capabilities schema.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ChannelSpecificCapabilitiesSchema {
    /// HTTP paths the channel can register for webhooks.
    #[serde(default)]
    pub allowed_paths: Vec<String>,

    /// Whether polling is allowed.
    #[serde(default)]
    pub allow_polling: bool,

    /// Minimum poll interval in milliseconds.
    #[serde(default)]
    pub min_poll_interval_ms: Option<u32>,

    /// Workspace prefix for storage (overrides default).
    #[serde(default)]
    pub workspace_prefix: Option<String>,

    /// Rate limiting for emit_message.
    #[serde(default)]
    pub emit_rate_limit: Option<EmitRateLimitSchema>,

    /// Maximum message content size in bytes.
    #[serde(default)]
    pub max_message_size: Option<usize>,

    /// Callback timeout in seconds.
    #[serde(default)]
    pub callback_timeout_secs: Option<u64>,

    /// Webhook configuration (secret header, etc.).
    #[serde(default)]
    pub webhook: Option<WebhookSchema>,
}

/// Webhook configuration schema.
///
/// Allows channels to specify their webhook validation requirements.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WebhookSchema {
    /// HTTP header name for secret validation.
    ///
    /// Examples:
    /// - Telegram: "X-Telegram-Bot-Api-Secret-Token"
    /// - Slack: "X-Slack-Signature"
    /// - GitHub: "X-Hub-Signature-256"
    /// - Generic: "X-Webhook-Secret"
    #[serde(default)]
    pub secret_header: Option<String>,

    /// Secret name in secrets store for webhook validation.
    /// Default: "{channel_name}_webhook_secret"
    #[serde(default)]
    pub secret_name: Option<String>,

    /// Secret name in secrets store containing the Ed25519 public key
    /// for signature verification (e.g., Discord interaction verification).
    #[serde(default)]
    pub signature_key_secret_name: Option<String>,

    /// Secret name in secrets store for HMAC-SHA256 signing (Slack-style).
    #[serde(default)]
    pub hmac_secret_name: Option<String>,
}

/// Schema for emit rate limiting.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EmitRateLimitSchema {
    /// Maximum messages per minute.
    #[serde(default = "default_messages_per_minute")]
    pub messages_per_minute: u32,

    /// Maximum messages per hour.
    #[serde(default = "default_messages_per_hour")]
    pub messages_per_hour: u32,
}

fn default_messages_per_minute() -> u32 {
    100
}

fn default_messages_per_hour() -> u32 {
    5000
}

impl EmitRateLimitSchema {
    fn to_emit_rate_limit(&self) -> EmitRateLimitConfig {
        EmitRateLimitConfig {
            messages_per_minute: self.messages_per_minute,
            messages_per_hour: self.messages_per_hour,
        }
    }
}

impl From<RateLimitSchema> for EmitRateLimitSchema {
    fn from(schema: RateLimitSchema) -> Self {
        Self {
            messages_per_minute: schema.requests_per_minute,
            messages_per_hour: schema.requests_per_hour,
        }
    }
}

#[cfg(test)]
mod tests;
