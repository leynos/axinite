//! Workspace persistence and emit rate limiting for WASM channels.
//!
//! Provides [`ChannelWorkspaceStore`] (in-memory workspace persisted across
//! callback invocations) and [`ChannelEmitRateLimiter`] (cross-execution
//! message emission limits).

use std::time::{SystemTime, UNIX_EPOCH};

use crate::channels::wasm::capabilities::EmitRateLimitConfig;

use super::message::PendingWorkspaceWrite;

/// In-memory workspace store for WASM channels.
///
/// Persists workspace writes across callback invocations within a single
/// channel lifetime. This allows WASM channels to maintain state (e.g.,
/// Telegram polling offsets) between poll ticks without requiring a
/// full database-backed workspace.
///
/// Uses `std::sync::RwLock` (not tokio) because WASM execution runs
/// inside `spawn_blocking`.
pub struct ChannelWorkspaceStore {
    data: std::sync::RwLock<std::collections::HashMap<String, String>>,
}

impl ChannelWorkspaceStore {
    /// Create a new empty workspace store.
    pub fn new() -> Self {
        Self {
            data: std::sync::RwLock::new(std::collections::HashMap::new()),
        }
    }

    /// Commit pending writes from a callback execution into the store.
    pub fn commit_writes(&self, writes: &[PendingWorkspaceWrite]) {
        if writes.is_empty() {
            return;
        }
        if let Ok(mut data) = self.data.write() {
            for write in writes {
                tracing::debug!(
                    path = %write.path,
                    content_len = write.content.len(),
                    "Committing workspace write to channel store"
                );
                data.insert(write.path.clone(), write.content.clone());
            }
        }
    }
}

impl crate::tools::wasm::WorkspaceReader for ChannelWorkspaceStore {
    fn read(&self, path: &str) -> Option<String> {
        self.data.read().ok()?.get(path).cloned()
    }
}

/// Rate limiter for channel message emission.
///
/// Tracks emission rates across multiple executions.
pub struct ChannelEmitRateLimiter {
    config: EmitRateLimitConfig,
    minute_window: RateWindow,
    hour_window: RateWindow,
}

struct RateWindow {
    count: u32,
    window_start: u64,
    window_duration_ms: u64,
}

impl RateWindow {
    fn new(duration_ms: u64) -> Self {
        Self {
            count: 0,
            window_start: 0,
            window_duration_ms: duration_ms,
        }
    }

    fn check_and_record(&mut self, now_ms: u64, limit: u32) -> bool {
        // Reset window if expired
        if now_ms.saturating_sub(self.window_start) > self.window_duration_ms {
            self.count = 0;
            self.window_start = now_ms;
        }

        if self.count >= limit {
            return false;
        }

        self.count += 1;
        true
    }
}

#[allow(dead_code)]
impl ChannelEmitRateLimiter {
    /// Create a new rate limiter with the given config.
    pub fn new(config: EmitRateLimitConfig) -> Self {
        Self {
            config,
            minute_window: RateWindow::new(60_000), // 1 minute
            hour_window: RateWindow::new(3_600_000), // 1 hour
        }
    }

    /// Check if an emit is allowed and record it if so.
    ///
    /// Returns true if the emit is allowed, false if rate limited.
    pub fn check_and_record(&mut self) -> bool {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0);

        // Check both windows
        let minute_ok = self
            .minute_window
            .check_and_record(now, self.config.messages_per_minute);
        let hour_ok = self
            .hour_window
            .check_and_record(now, self.config.messages_per_hour);

        minute_ok && hour_ok
    }

    /// Get the current emission count for the minute window.
    pub fn minute_count(&self) -> u32 {
        self.minute_window.count
    }

    /// Get the current emission count for the hour window.
    pub fn hour_count(&self) -> u32 {
        self.hour_window.count
    }
}
