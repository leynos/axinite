//! Per-provider cooldown configuration and lock-free cooldown state.

use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use std::time::Duration;

/// Configuration for per-provider cooldown behaviour.
///
/// When a provider accumulates `failure_threshold` consecutive retryable
/// failures, it enters cooldown for `cooldown_duration`. During cooldown
/// the provider is skipped (unless *all* providers are in cooldown, in
/// which case the oldest-cooled one is tried).
#[derive(Debug, Clone)]
pub struct CooldownConfig {
    /// How long a provider stays in cooldown after exceeding the threshold.
    pub cooldown_duration: Duration,
    /// Number of consecutive retryable failures before cooldown activates.
    pub failure_threshold: u32,
}

impl Default for CooldownConfig {
    fn default() -> Self {
        Self {
            cooldown_duration: Duration::from_secs(300),
            failure_threshold: 3,
        }
    }
}

/// Per-provider cooldown state, entirely lock-free.
///
/// All atomic operations use `Relaxed` ordering — consistent with the
/// existing `last_used` field. Stale reads are harmless: the worst case
/// is one extra attempt against a provider that just entered cooldown.
pub(super) struct ProviderCooldown {
    /// Consecutive retryable failures. Reset to 0 on success.
    pub(super) failure_count: AtomicU32,
    /// Nanoseconds since `epoch` when cooldown was activated.
    /// 0 means the provider is NOT in cooldown.
    pub(super) cooldown_activated_nanos: AtomicU64,
}

impl ProviderCooldown {
    pub(super) fn new() -> Self {
        Self {
            failure_count: AtomicU32::new(0),
            cooldown_activated_nanos: AtomicU64::new(0),
        }
    }

    /// Check whether the provider is currently in cooldown.
    pub(super) fn is_in_cooldown(&self, now_nanos: u64, cooldown_nanos: u64) -> bool {
        let activated = self.cooldown_activated_nanos.load(Ordering::Relaxed);
        activated != 0 && now_nanos.saturating_sub(activated) < cooldown_nanos
    }

    /// Record a retryable failure. Returns `true` if the threshold was
    /// just reached (caller should activate cooldown).
    pub(super) fn record_failure(&self, threshold: u32) -> bool {
        let prev = self.failure_count.fetch_add(1, Ordering::Relaxed);
        prev + 1 >= threshold
    }

    /// Activate cooldown at the given timestamp.
    pub(super) fn activate_cooldown(&self, now_nanos: u64) {
        // Ensure 0 remains a safe "not in cooldown" sentinel.
        self.cooldown_activated_nanos
            .store(now_nanos.max(1), Ordering::Relaxed);
    }

    /// Reset failure count and clear cooldown (called on success).
    pub(super) fn reset(&self) {
        self.failure_count.store(0, Ordering::Relaxed);
        self.cooldown_activated_nanos.store(0, Ordering::Relaxed);
    }
}
