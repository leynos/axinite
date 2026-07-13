//! Settings sections for embeddings, tunnels, channels, and heartbeat.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use super::default_true;

/// Embeddings configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EmbeddingsSettings {
    /// Whether embeddings are enabled.
    #[serde(default)]
    pub enabled: bool,

    /// Provider to use: "openai" or "nearai".
    #[serde(default = "default_embeddings_provider")]
    pub provider: String,

    /// Model to use for embeddings.
    #[serde(default = "default_embeddings_model")]
    pub model: String,
}

fn default_embeddings_provider() -> String {
    "nearai".to_string()
}

fn default_embeddings_model() -> String {
    "text-embedding-3-small".to_string()
}

impl Default for EmbeddingsSettings {
    fn default() -> Self {
        Self {
            enabled: false,
            provider: default_embeddings_provider(),
            model: default_embeddings_model(),
        }
    }
}

/// Tunnel settings for public webhook endpoints.
///
/// The tunnel URL is shared across all channels that need webhooks.
/// Two modes:
/// - **Static URL**: `public_url` set directly (manual tunnel management).
/// - **Managed provider**: `provider` is set and the agent starts/stops the
///   tunnel process automatically at boot/shutdown.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct TunnelSettings {
    /// Public URL from tunnel provider (e.g., "https://abc123.ngrok.io").
    /// When set without a provider, treated as a static (externally managed) URL.
    #[serde(default)]
    pub public_url: Option<String>,

    /// Managed tunnel provider: "ngrok", "cloudflare", "tailscale", "custom".
    #[serde(default)]
    pub provider: Option<String>,

    /// Cloudflare tunnel token.
    #[serde(default)]
    pub cf_token: Option<String>,

    /// ngrok auth token.
    #[serde(default)]
    pub ngrok_token: Option<String>,

    /// ngrok custom domain (paid plans).
    #[serde(default)]
    pub ngrok_domain: Option<String>,

    /// Use Tailscale Funnel (public) instead of Serve (tailnet-only).
    #[serde(default)]
    pub ts_funnel: bool,

    /// Tailscale hostname override.
    #[serde(default)]
    pub ts_hostname: Option<String>,

    /// Shell command for custom tunnel (with `{port}` / `{host}` placeholders).
    #[serde(default)]
    pub custom_command: Option<String>,

    /// Health check URL for custom tunnel.
    #[serde(default)]
    pub custom_health_url: Option<String>,

    /// Substring pattern to extract URL from custom tunnel stdout.
    #[serde(default)]
    pub custom_url_pattern: Option<String>,
}

/// Channel-specific settings.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ChannelSettings {
    /// Whether HTTP webhook channel is enabled.
    #[serde(default)]
    pub http_enabled: bool,

    /// HTTP webhook port (if enabled).
    #[serde(default)]
    pub http_port: Option<u16>,

    /// HTTP webhook host.
    #[serde(default)]
    pub http_host: Option<String>,

    /// Whether Signal channel is enabled.
    #[serde(default)]
    pub signal_enabled: bool,

    /// Signal HTTP URL (signal-cli daemon endpoint).
    #[serde(default)]
    pub signal_http_url: Option<String>,

    /// Signal account (E.164 phone number).
    #[serde(default)]
    pub signal_account: Option<String>,

    /// Signal allow from list for DMs (comma-separated E.164 phone numbers).
    /// Comma-separated identifiers: E.164 phone numbers, `*`, bare UUIDs, or `uuid:<id>` entries.
    /// Defaults to the configured account.
    #[serde(default)]
    pub signal_allow_from: Option<String>,

    /// Signal allow from groups (comma-separated group IDs).
    #[serde(default)]
    pub signal_allow_from_groups: Option<String>,

    /// Signal DM policy: "open", "allowlist", or "pairing". Default: "pairing".
    #[serde(default)]
    pub signal_dm_policy: Option<String>,

    /// Signal group policy: "allowlist", "open", or "disabled". Default: "allowlist".
    #[serde(default)]
    pub signal_group_policy: Option<String>,

    /// Signal group allow from (comma-separated group member IDs).
    /// If empty, inherits from signal_allow_from.
    #[serde(default)]
    pub signal_group_allow_from: Option<String>,

    /// Per-channel owner user IDs. When set, the channel only responds to this user.
    /// Key: channel name (e.g., "telegram"), Value: owner user ID.
    #[serde(default)]
    pub wasm_channel_owner_ids: std::collections::HashMap<String, i64>,

    /// Enabled WASM channels by name.
    /// Channels not in this list but present in the channels directory will still load.
    /// This is primarily used by the setup wizard to track which channels were configured.
    #[serde(default)]
    pub wasm_channels: Vec<String>,

    /// Whether WASM channels are enabled.
    #[serde(default = "default_true")]
    pub wasm_channels_enabled: bool,

    /// Directory containing WASM channel modules.
    #[serde(default)]
    pub wasm_channels_dir: Option<PathBuf>,
}

/// Heartbeat configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HeartbeatSettings {
    /// Whether heartbeat is enabled.
    #[serde(default)]
    pub enabled: bool,

    /// Interval between heartbeat checks in seconds.
    #[serde(default = "default_heartbeat_interval")]
    pub interval_secs: u64,

    /// Channel to notify on heartbeat findings.
    #[serde(default)]
    pub notify_channel: Option<String>,

    /// User ID to notify on heartbeat findings.
    #[serde(default)]
    pub notify_user: Option<String>,

    /// Hour (0-23) when quiet hours start (heartbeat skipped).
    #[serde(default)]
    pub quiet_hours_start: Option<u32>,

    /// Hour (0-23) when quiet hours end (heartbeat resumes).
    #[serde(default)]
    pub quiet_hours_end: Option<u32>,

    /// Timezone for quiet hours evaluation (IANA name, e.g. "America/New_York").
    #[serde(default)]
    pub timezone: Option<String>,
}

fn default_heartbeat_interval() -> u64 {
    1800 // 30 minutes
}

impl Default for HeartbeatSettings {
    fn default() -> Self {
        Self {
            enabled: false,
            interval_secs: default_heartbeat_interval(),
            notify_channel: None,
            notify_user: None,
            quiet_hours_start: None,
            quiet_hours_end: None,
            timezone: None,
        }
    }
}
