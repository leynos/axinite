//! Signal channel via signal-cli daemon HTTP/JSON-RPC.
//!
//! Connects to a running `signal-cli daemon --http <host:port>`.
//! Listens for messages via SSE at `/api/v1/events` and sends via
//! JSON-RPC at `/api/v1/rpc`.
//!
//! This module is split into focused submodules:
//! - [`incoming`] — SSE envelope processing into `IncomingMessage`s
//! - [`listener`] — the long-running SSE listener task
//! - [`native`] — the `NativeChannel` trait implementation
//! - [`pairing`] — pairing-store integration and pairing replies
//! - [`policy`] — sender/group allowlists and DM/group policies
//! - [`rpc`] — JSON-RPC requests and send-parameter construction

mod incoming;
mod listener;
mod native;
mod pairing;
mod policy;
mod rpc;

#[cfg(test)]
mod tests;

use std::num::NonZeroUsize;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use lru::LruCache;
use reqwest::Client;
use serde::Deserialize;
use tokio::sync::RwLock;
use uuid::Uuid;

use crate::config::SignalConfig;
use crate::error::ChannelError;

use listener::sse_listener;

const GROUP_TARGET_PREFIX: &str = "group:";
const SIGNAL_HEALTH_ENDPOINT: &str = "/api/v1/check";

const MAX_SSE_BUFFER_SIZE: usize = 1024 * 1024;
const MAX_SSE_EVENT_SIZE: usize = 256 * 1024;
const MAX_HTTP_RESPONSE_SIZE: usize = 10 * 1024 * 1024;
const MAX_REPLY_TARGETS: usize = 10000;
const MAX_ERROR_LOG_BODY: usize = 1024;

const REPLY_TARGETS_CAP: NonZeroUsize = NonZeroUsize::new(MAX_REPLY_TARGETS).unwrap();

/// Recipient classification for outbound messages.
#[derive(Debug, Clone, PartialEq, Eq)]
enum RecipientTarget {
    Direct(String),
    Group(String),
}

// ── signal-cli SSE event JSON shapes ────────────────────────────

#[derive(Debug, Deserialize)]
struct SseEnvelope {
    #[serde(default)]
    envelope: Option<Envelope>,
}

#[derive(Debug, Deserialize)]
struct Envelope {
    #[serde(default)]
    source: Option<String>,
    #[serde(rename = "sourceNumber", default)]
    source_number: Option<String>,
    #[serde(rename = "sourceName", default)]
    source_name: Option<String>,
    #[serde(rename = "sourceUuid", default)]
    source_uuid: Option<String>,
    #[serde(rename = "dataMessage", default)]
    data_message: Option<DataMessage>,
    #[serde(rename = "storyMessage", default)]
    story_message: Option<serde_json::Value>,
    #[serde(default)]
    timestamp: Option<u64>,
}

#[derive(Debug, Deserialize)]
struct DataMessage {
    #[serde(default)]
    message: Option<String>,
    #[serde(default)]
    timestamp: Option<u64>,
    #[serde(rename = "groupInfo", default)]
    group_info: Option<GroupInfo>,
    #[serde(default)]
    attachments: Option<Vec<serde_json::Value>>,
}

#[derive(Debug, Deserialize)]
struct GroupInfo {
    #[serde(rename = "groupId", default)]
    group_id: Option<String>,
}

/// Signal channel using signal-cli daemon's native JSON-RPC + SSE API.
pub struct SignalChannel {
    config: SignalConfig,
    client: Client,
    /// LRU cache of reply targets per incoming message, used by `respond()`.
    /// Bounded to `MAX_REPLY_TARGETS` entries; least-recently-used entries
    /// are evicted automatically when the cache is full.
    reply_targets: Arc<RwLock<LruCache<Uuid, String>>>,
    /// Debug mode for verbose tool output (toggled via /debug command).
    debug_mode: Arc<AtomicBool>,
}

impl SignalChannel {
    /// Create a new Signal channel with normalized config and fresh client/cache.
    pub fn new(config: SignalConfig) -> Result<Self, ChannelError> {
        let mut config = config;
        config.http_url = config.http_url.trim_end_matches('/').to_string();

        let client = Client::builder()
            .connect_timeout(Duration::from_secs(10))
            .build()
            .map_err(|e| ChannelError::Http(e.to_string()))?;

        let cap = REPLY_TARGETS_CAP;
        let reply_targets = Arc::new(RwLock::new(LruCache::new(cap)));
        let debug_mode = Arc::new(AtomicBool::new(false));

        Ok(Self::from_parts(config, client, reply_targets, debug_mode))
    }

    /// Construct a SignalChannel from pre-validated parts.
    ///
    /// Used by [`new()`][Self::new] after normalization and by [`sse_listener`]
    /// to ensure both code paths use the same constructor.
    fn from_parts(
        config: SignalConfig,
        client: Client,
        reply_targets: Arc<RwLock<LruCache<Uuid, String>>>,
        debug_mode: Arc<AtomicBool>,
    ) -> Self {
        Self {
            config,
            client,
            reply_targets,
            debug_mode,
        }
    }

    fn is_debug(&self) -> bool {
        self.debug_mode.load(Ordering::Relaxed)
    }

    fn toggle_debug(&self) -> bool {
        let current = self.debug_mode.load(Ordering::Relaxed);
        self.debug_mode.store(!current, Ordering::Relaxed);
        !current
    }
}

#[cfg(test)]
static SIGNAL_PAIRING_STORE_OVERRIDE: std::sync::OnceLock<
    std::sync::Mutex<Option<std::path::PathBuf>>,
> = std::sync::OnceLock::new();
