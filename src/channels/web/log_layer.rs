//! Tracing layer that broadcasts log events to the web gateway via SSE.
//!
//! ```text
//! tracing::info!("...")
//!        │
//!        ▼
//!   WebLogLayer::on_event()
//!        │
//!        ▼
//!   LogBroadcaster::send()
//!        │
//!        ├──► broadcast::Sender<LogEntry>  (live subscribers)
//!        └──► ring buffer (recent history for late joiners)
//!                   │
//!                   ▼
//!             SSE /api/logs/events
//! ```

use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

use serde::Serialize;
use tokio::sync::broadcast;
use tracing::field::{Field, Visit};
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;
use tracing_subscriber::{EnvFilter, Layer, reload};

use crate::safety::LeakDetector;

/// Maximum number of recent log entries kept for late-joining SSE subscribers.
const HISTORY_CAP: usize = 500;

/// A single log entry broadcast to connected clients.
#[derive(Debug, Clone, Serialize)]
pub struct LogEntry {
    pub level: String,
    pub target: String,
    pub message: String,
    pub timestamp: String,
}

/// Broadcasts log entries to SSE subscribers.
///
/// Created early in main.rs (before tracing init), shared with both
/// the tracing layer and the gateway's SSE endpoint.
///
/// Keeps a ring buffer of recent entries so browsers that connect
/// after startup still see the boot log.
pub struct LogBroadcaster {
    tx: broadcast::Sender<LogEntry>,
    recent: Mutex<VecDeque<LogEntry>>,
    /// Scrubs secrets from log messages before broadcasting to SSE clients.
    leak_detector: LeakDetector,
}

impl LogBroadcaster {
    pub fn new() -> Self {
        let (tx, _) = broadcast::channel(512);
        Self {
            tx,
            recent: Mutex::new(VecDeque::with_capacity(HISTORY_CAP)),
            leak_detector: LeakDetector::new(),
        }
    }

    pub fn send(&self, mut entry: LogEntry) {
        // Scrub secrets from the message before it reaches any subscriber.
        // This is defence-in-depth: even if code elsewhere accidentally logs
        // a secret, it won't be broadcast to SSE clients.
        entry.message = self
            .leak_detector
            .scan_and_clean(&entry.message)
            .unwrap_or_else(|_| "[log message redacted: contained blocked secret]".to_string());

        // Stash in ring buffer (for late joiners)
        if let Ok(mut buf) = self.recent.lock() {
            if buf.len() >= HISTORY_CAP {
                buf.pop_front();
            }
            buf.push_back(entry.clone());
        }
        // Broadcast to live subscribers (ok to drop if nobody listening)
        let _ = self.tx.send(entry);
    }

    /// Subscribe to the live event stream.
    pub fn subscribe(&self) -> broadcast::Receiver<LogEntry> {
        self.tx.subscribe()
    }

    /// Snapshot of recent entries for replaying to a new subscriber.
    ///
    /// Returns entries oldest-first so that the frontend's `prepend()`
    /// naturally places the newest entry at the top of the DOM.
    pub fn recent_entries(&self) -> Vec<LogEntry> {
        self.recent
            .lock()
            .map(|buf| buf.iter().cloned().collect())
            .unwrap_or_default()
    }
}

impl Default for LogBroadcaster {
    fn default() -> Self {
        Self::new()
    }
}

/// Handle for changing the tracing `EnvFilter` at runtime.
///
/// Wraps a `reload::Handle` so the gateway can switch between log levels
/// (e.g. `ironclaw=debug`) without restarting the process.
pub struct LogLevelHandle {
    handle: reload::Handle<EnvFilter, tracing_subscriber::Registry>,
    current_level: Mutex<String>,
    base_filter: String,
}

impl LogLevelHandle {
    pub fn new(
        handle: reload::Handle<EnvFilter, tracing_subscriber::Registry>,
        initial_level: String,
        base_filter: String,
    ) -> Self {
        Self {
            handle,
            current_level: Mutex::new(initial_level),
            base_filter,
        }
    }

    /// Change the `ironclaw=<level>` directive at runtime.
    ///
    /// `level` must be one of: trace, debug, info, warn, error.
    pub fn set_level(&self, level: &str) -> Result<(), String> {
        const VALID: &[&str] = &["trace", "debug", "info", "warn", "error"];
        let level = level.to_lowercase();
        if !VALID.contains(&level.as_str()) {
            return Err(format!(
                "invalid level '{}', must be one of: {}",
                level,
                VALID.join(", ")
            ));
        }

        let filter_str = if self.base_filter.is_empty() {
            format!("ironclaw={}", level)
        } else {
            format!("ironclaw={},{}", level, self.base_filter)
        };

        let new_filter = EnvFilter::new(&filter_str);
        self.handle
            .reload(new_filter)
            .map_err(|e| format!("failed to reload filter: {}", e))?;

        if let Ok(mut current) = self.current_level.lock() {
            *current = level;
        }
        Ok(())
    }

    /// Returns the current ironclaw log level (e.g. "info", "debug").
    pub fn current_level(&self) -> String {
        self.current_level
            .lock()
            .map(|l| l.clone())
            .unwrap_or_else(|_| "info".to_string())
    }
}

/// Initialize the tracing subscriber with a reloadable `EnvFilter`.
///
/// Returns the `LogLevelHandle` so callers can swap the filter at runtime.
/// The fmt layer and `WebLogLayer` are attached alongside the reloadable filter.
pub fn init_tracing(log_broadcaster: Arc<LogBroadcaster>) -> Arc<LogLevelHandle> {
    let raw_filter =
        std::env::var("RUST_LOG").unwrap_or_else(|_| "ironclaw=info,tower_http=warn".to_string());

    // Split into the ironclaw directive and "everything else" (base_filter).
    let mut ironclaw_level = String::from("info");
    let mut base_parts: Vec<&str> = Vec::new();

    for part in raw_filter.split(',') {
        let trimmed = part.trim();
        if trimmed.starts_with("ironclaw=") {
            if let Some(lvl) = trimmed.strip_prefix("ironclaw=") {
                ironclaw_level = lvl.to_string();
            }
        } else if !trimmed.is_empty() {
            base_parts.push(trimmed);
        }
    }
    let base_filter = base_parts.join(",");

    let env_filter = EnvFilter::new(&raw_filter);
    let (reload_layer, reload_handle) = reload::Layer::new(env_filter);

    let handle = Arc::new(LogLevelHandle::new(
        reload_handle,
        ironclaw_level,
        base_filter,
    ));

    tracing_subscriber::registry()
        .with(reload_layer)
        .with(
            tracing_subscriber::fmt::layer()
                .with_target(false)
                .with_writer(crate::tracing_fmt::TruncatingStderr::default()),
        )
        .with(WebLogLayer::new(log_broadcaster))
        .try_init()
        .ok();

    handle
}

/// Visitor that extracts the `message` field and all extra key-value
/// fields from a tracing event.
///
/// The terminal formatter shows something like:
///   INFO ironclaw::agent: Request completed url="http://..." status=200
///
/// We replicate that by capturing both the message and the extra fields.
struct MessageVisitor {
    message: String,
    fields: Vec<String>,
}

impl MessageVisitor {
    fn new() -> Self {
        Self {
            message: String::new(),
            fields: Vec::new(),
        }
    }

    /// Build the final message string: "message key=val key=val ..."
    fn finish(self) -> String {
        if self.fields.is_empty() {
            self.message
        } else {
            format!("{} {}", self.message, self.fields.join(" "))
        }
    }
}

impl Visit for MessageVisitor {
    fn record_debug(&mut self, field: &Field, value: &dyn std::fmt::Debug) {
        if field.name() == "message" {
            self.message = format!("{:?}", value);
            // Strip surrounding quotes from Debug output
            if self.message.starts_with('"') && self.message.ends_with('"') {
                self.message = self.message[1..self.message.len() - 1].to_string();
            }
        } else {
            self.fields.push(format!("{}={:?}", field.name(), value));
        }
    }

    fn record_str(&mut self, field: &Field, value: &str) {
        if field.name() == "message" {
            self.message = value.to_string();
        } else {
            self.fields.push(format!("{}={}", field.name(), value));
        }
    }
}

/// Tracing layer that forwards events to a [`LogBroadcaster`].
///
/// Only forwards DEBUG and above. Attach to the tracing subscriber
/// alongside the existing fmt layer.
///
/// Log messages are scrubbed through `LeakDetector` in `LogBroadcaster::send()`
/// (the single funnel point for all log output, including late-joiner history).
pub struct WebLogLayer {
    broadcaster: Arc<LogBroadcaster>,
}

impl WebLogLayer {
    pub fn new(broadcaster: Arc<LogBroadcaster>) -> Self {
        Self { broadcaster }
    }
}

impl<S: tracing::Subscriber> Layer<S> for WebLogLayer {
    fn on_event(
        &self,
        event: &tracing::Event<'_>,
        _ctx: tracing_subscriber::layer::Context<'_, S>,
    ) {
        let metadata = event.metadata();

        // Only forward DEBUG+
        if *metadata.level() > tracing::Level::DEBUG {
            return;
        }

        let mut visitor = MessageVisitor::new();
        event.record(&mut visitor);

        let entry = LogEntry {
            level: metadata.level().to_string().to_uppercase(),
            target: metadata.target().to_string(),
            message: visitor.finish(),
            timestamp: chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true),
        };

        // LeakDetector scrubbing happens inside broadcaster.send()
        self.broadcaster.send(entry);
    }
}

#[cfg(test)]
mod tests;
