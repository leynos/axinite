//! Channel trait implementation for channel-relay SSE streams.
//!
//! `RelayChannel` connects to a channel-relay service via SSE, converts
//! incoming events to `IncomingMessage`s, and sends responses via the
//! relay's provider-specific proxy API (Slack).

use std::sync::Arc;

use tokio::sync::RwLock;

use crate::channels::relay::client::{RelayClient, RelayError};

/// Default channel name for the Slack relay integration.
pub const DEFAULT_RELAY_NAME: &str = "slack-relay";

/// The messaging provider backing a relay channel.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RelayProvider {
    Slack,
}

impl RelayProvider {
    /// Provider string used in proxy API routes and metadata.
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Slack => "slack",
        }
    }

    /// The default channel name for this provider.
    pub fn channel_name(&self) -> &'static str {
        match self {
            Self::Slack => DEFAULT_RELAY_NAME,
        }
    }
}

/// Identity and credentials binding a relay channel to one stream.
pub struct RelayIdentity {
    /// Token authorizing the SSE stream connection.
    pub stream_token: String,
    /// Workspace/team the relay is bound to.
    pub team_id: String,
    /// Relay instance identifier.
    pub instance_id: String,
    /// User the relay acts on behalf of.
    pub user_id: String,
}

/// Channel implementation that connects to a channel-relay SSE stream.
pub struct RelayChannel {
    client: RelayClient,
    provider: RelayProvider,
    stream_token: Arc<RwLock<String>>,
    team_id: String,
    instance_id: String,
    user_id: String,
    /// SSE stream long-poll timeout in seconds.
    stream_timeout_secs: u64,
    /// Initial exponential backoff in milliseconds.
    backoff_initial_ms: u64,
    /// Maximum exponential backoff in milliseconds.
    backoff_max_ms: u64,
    /// Handle to the reconnect task for clean shutdown.
    reconnect_handle: RwLock<Option<tokio::task::JoinHandle<()>>>,
    /// Handle to the SSE parser task for clean shutdown.
    parser_handle: Arc<RwLock<Option<tokio::task::JoinHandle<()>>>>,
    /// Maximum consecutive reconnect failures before giving up.
    max_consecutive_failures: u64,
}

impl RelayChannel {
    /// Create a new relay channel for Slack (default provider).
    pub fn new(client: RelayClient, identity: RelayIdentity) -> Self {
        Self::new_with_provider(client, RelayProvider::Slack, identity)
    }

    /// Create a new relay channel with a specific provider.
    pub fn new_with_provider(
        client: RelayClient,
        provider: RelayProvider,
        identity: RelayIdentity,
    ) -> Self {
        Self {
            client,
            provider,
            stream_token: Arc::new(RwLock::new(identity.stream_token)),
            team_id: identity.team_id,
            instance_id: identity.instance_id,
            user_id: identity.user_id,
            stream_timeout_secs: 86400,
            backoff_initial_ms: 1000,
            backoff_max_ms: 60000,
            reconnect_handle: RwLock::new(None),
            parser_handle: Arc::new(RwLock::new(None)),
            max_consecutive_failures: 50,
        }
    }

    /// Set backoff/timeout parameters from relay config values.
    pub fn with_timeouts(
        mut self,
        stream_timeout_secs: u64,
        backoff_initial_ms: u64,
        backoff_max_ms: u64,
    ) -> Self {
        self.stream_timeout_secs = stream_timeout_secs;
        self.backoff_initial_ms = backoff_initial_ms;
        self.backoff_max_ms = backoff_max_ms;
        self
    }

    /// Set the maximum number of consecutive reconnect failures before giving up.
    pub fn with_max_failures(mut self, max: u64) -> Self {
        self.max_consecutive_failures = max;
        self
    }

    /// Build a provider-appropriate proxy body for sending a message.
    fn build_send_body(
        &self,
        channel_id: &str,
        text: &str,
        thread_id: Option<&str>,
    ) -> (String, serde_json::Value) {
        match self.provider {
            RelayProvider::Slack => {
                let mut body = serde_json::json!({
                    "channel": channel_id,
                    "text": text,
                });
                if let Some(tid) = thread_id {
                    body["thread_ts"] = serde_json::Value::String(tid.to_string());
                }
                ("chat.postMessage".to_string(), body)
            }
        }
    }

    /// Send a message via the provider proxy.
    async fn proxy_send(
        &self,
        team_id: &str,
        method: &str,
        body: serde_json::Value,
    ) -> Result<serde_json::Value, RelayError> {
        self.client
            .proxy_provider(
                self.provider.as_str(),
                team_id,
                method,
                body,
                Some(&self.instance_id),
            )
            .await
    }
}
mod native;
mod stream_task;

#[cfg(test)]
mod tests;
