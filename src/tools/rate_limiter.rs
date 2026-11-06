//! Shared rate limiter for built-in and WASM tool invocations.
//!
//! Provides per-tool, per-user rate limiting using a sliding window counter.
//! Built-in tools (shell, http, file write, etc.) are throttled here before
//! `tool.execute()` is called in the agent loop. WASM tools re-export these
//! types for HTTP-level rate limiting inside host functions.
//!
//! # Rate Limit Algorithm
//!
//! Uses a simplified sliding window counter:
//! - Track request counts for current minute and hour windows
//! - Reset counters when window expires
//! - Increment counter and check against limits
//!
//! # Persistence
//!
//! Rate limit state is in-memory only. Limits reset on process restart.
//! This is acceptable for v1; future versions may persist to the database.

use std::collections::HashMap;
use std::time::{Duration, Instant};

use tokio::sync::RwLock;

use crate::tools::tool::ToolRateLimitConfig;

const MINUTE_SECS: u64 = 60;
const HOUR_SECS: u64 = 3600;

/// Result of a rate limit check.
#[derive(Debug, Clone)]
pub enum RateLimitResult {
    /// Request is allowed.
    Allowed {
        /// Remaining requests in the current minute.
        remaining_minute: u32,
        /// Remaining requests in the current hour.
        remaining_hour: u32,
    },
    /// Request is rate limited.
    Limited {
        /// When the rate limit will reset.
        retry_after: Duration,
        /// Which limit was exceeded.
        limit_type: LimitType,
    },
}

impl RateLimitResult {
    /// Whether the request passed the check and may proceed.
    pub fn is_allowed(&self) -> bool {
        matches!(self, RateLimitResult::Allowed { .. })
    }
}

/// Which rate limit was exceeded.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LimitType {
    /// The 60-second window's request budget was used up.
    PerMinute,
    /// The 3600-second window's request budget was used up.
    PerHour,
}

impl std::fmt::Display for LimitType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LimitType::PerMinute => write!(f, "per-minute"),
            LimitType::PerHour => write!(f, "per-hour"),
        }
    }
}

/// State for a single rate limit window.
#[derive(Debug, Clone)]
struct WindowState {
    window_start: Instant,
    count: u32,
}

impl WindowState {
    fn new() -> Self {
        Self {
            window_start: Instant::now(),
            count: 0,
        }
    }

    /// Check if the window has expired and reset if needed.
    fn maybe_reset(&mut self, window_duration: Duration) {
        if self.window_start.elapsed() >= window_duration {
            self.window_start = Instant::now();
            self.count = 0;
        }
    }

    /// Time until window resets.
    fn time_until_reset(&self, window_duration: Duration) -> Duration {
        let elapsed = self.window_start.elapsed();
        if elapsed >= window_duration {
            Duration::ZERO
        } else {
            window_duration - elapsed
        }
    }
}

/// Rate limit state for a single (user, tool) pair.
#[derive(Debug)]
struct ToolRateLimitState {
    minute_window: WindowState,
    hour_window: WindowState,
}

impl ToolRateLimitState {
    fn new() -> Self {
        Self {
            minute_window: WindowState::new(),
            hour_window: WindowState::new(),
        }
    }
}

/// In-memory rate limiter for tool invocations.
///
/// Keyed by `(user_id, tool_name)` so each user has independent limits.
/// Shared via `Arc` — a single instance lives in `ToolRegistry` and is
/// checked before every built-in tool execution.
pub struct RateLimiter {
    state: RwLock<HashMap<(String, String), ToolRateLimitState>>,
}

impl RateLimiter {
    /// Create a new rate limiter.
    pub fn new() -> Self {
        Self {
            state: RwLock::new(HashMap::new()),
        }
    }

    /// Shared logic: reset windows, check limits, and optionally record the request.
    async fn check_internal(
        &self,
        user_id: &str,
        tool_name: &str,
        config: &ToolRateLimitConfig,
        record: bool,
    ) -> RateLimitResult {
        let key = (user_id.to_string(), tool_name.to_string());

        let mut state = self.state.write().await;
        let tool_state = state.entry(key).or_insert_with(ToolRateLimitState::new);

        // Reset windows if expired.
        tool_state
            .minute_window
            .maybe_reset(Duration::from_secs(MINUTE_SECS));
        tool_state
            .hour_window
            .maybe_reset(Duration::from_secs(HOUR_SECS));

        // Check minute limit.
        if tool_state.minute_window.count >= config.requests_per_minute {
            return RateLimitResult::Limited {
                retry_after: tool_state
                    .minute_window
                    .time_until_reset(Duration::from_secs(MINUTE_SECS)),
                limit_type: LimitType::PerMinute,
            };
        }

        // Check hour limit.
        if tool_state.hour_window.count >= config.requests_per_hour {
            return RateLimitResult::Limited {
                retry_after: tool_state
                    .hour_window
                    .time_until_reset(Duration::from_secs(HOUR_SECS)),
                limit_type: LimitType::PerHour,
            };
        }

        if record {
            tool_state.minute_window.count += 1;
            tool_state.hour_window.count += 1;
        }

        RateLimitResult::Allowed {
            remaining_minute: config.requests_per_minute - tool_state.minute_window.count,
            remaining_hour: config.requests_per_hour - tool_state.hour_window.count,
        }
    }

    /// Check if a request is allowed and record it if so.
    pub async fn check_and_record(
        &self,
        user_id: &str,
        tool_name: &str,
        config: &ToolRateLimitConfig,
    ) -> RateLimitResult {
        self.check_internal(user_id, tool_name, config, true).await
    }

    /// Check without recording (for preview/estimation).
    pub async fn check(
        &self,
        user_id: &str,
        tool_name: &str,
        config: &ToolRateLimitConfig,
    ) -> RateLimitResult {
        self.check_internal(user_id, tool_name, config, false).await
    }

    /// Get current usage for a (user, tool) pair.
    pub async fn get_usage(&self, user_id: &str, tool_name: &str) -> Option<(u32, u32)> {
        let key = (user_id.to_string(), tool_name.to_string());
        let state = self.state.read().await;
        state
            .get(&key)
            .map(|s| (s.minute_window.count, s.hour_window.count))
    }

    /// Clear rate limit state for a specific (user, tool) pair.
    pub async fn clear(&self, user_id: &str, tool_name: &str) {
        let key = (user_id.to_string(), tool_name.to_string());
        self.state.write().await.remove(&key);
    }

    /// Clear all rate limit state.
    pub async fn clear_all(&self) {
        self.state.write().await.clear();
    }
}

impl Default for RateLimiter {
    fn default() -> Self {
        Self::new()
    }
}

/// Error when rate limited.
#[derive(Debug, Clone, thiserror::Error)]
#[error("Rate limited ({limit_type}), retry after {retry_after:?}")]
pub struct RateLimitError {
    /// Time until the exhausted window resets; the earliest a retry can succeed.
    pub retry_after: Duration,
    /// The window whose budget was exhausted.
    pub limit_type: LimitType,
}

impl From<RateLimitResult> for Result<(), RateLimitError> {
    fn from(result: RateLimitResult) -> Self {
        match result {
            RateLimitResult::Allowed { .. } => Ok(()),
            RateLimitResult::Limited {
                retry_after,
                limit_type,
            } => Err(RateLimitError {
                retry_after,
                limit_type,
            }),
        }
    }
}

#[cfg(test)]
mod tests;
