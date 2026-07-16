//! In-memory LLM response cache with TTL and LRU eviction.
//!
//! Wraps any [`LlmProvider`] and caches [`complete()`] responses keyed
//! by a SHA-256 hash of the messages and model name. Tool-calling
//! requests are never cached since they can trigger side effects.
//!
//! ```text
//! ┌──────────────────────────────────────────────────┐
//! │               CachedProvider                      │
//! │  complete() ──► cache lookup ──► hit? return      │
//! │                                  miss? call inner │
//! │                                  store response   │
//! │                                                    │
//! │  complete_with_tools() ──► always call inner       │
//! └──────────────────────────────────────────────────┘
//! ```

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use std::time::{Duration, Instant};

use rust_decimal::Decimal;
use sha2::{Digest, Sha256};

use crate::llm::error::LlmError;
use crate::llm::provider::{
    CompletionRequest, CompletionResponse, LlmProvider, ModelMetadata, ToolCompletionRequest,
    ToolCompletionResponse,
};

/// How often (in requests) to emit a cache statistics log line.
const STATS_LOG_EVERY_N: u64 = 100;

/// Configuration for the response cache.
#[derive(Debug, Clone)]
pub struct ResponseCacheConfig {
    /// Time-to-live for cache entries.
    pub ttl: Duration,
    /// Maximum number of cached entries before LRU eviction.
    pub max_entries: usize,
}

impl Default for ResponseCacheConfig {
    fn default() -> Self {
        Self {
            ttl: Duration::from_secs(3600), // 1 hour
            max_entries: 1000,
        }
    }
}

struct CacheEntry {
    response: CompletionResponse,
    created_at: Instant,
    last_accessed: Instant,
    hit_count: u64,
}

/// LLM provider wrapper that caches `complete()` responses.
///
/// Tool completion requests are always forwarded without caching since
/// tool calls can have side effects that should not be replayed.
pub struct CachedProvider {
    inner: Arc<dyn LlmProvider>,
    /// `std::sync::Mutex` (not tokio) — never held across an `.await` point,
    /// so blocking acquisition is safe and keeps `set_model()` synchronous.
    cache: Mutex<HashMap<String, CacheEntry>>,
    config: ResponseCacheConfig,
    /// Total `complete()` calls (hits + misses) for periodic stats logging.
    request_count: AtomicU64,
    /// Running total of cache hits, independent of entry lifecycle.
    /// Never decremented on eviction, so `hit_rate_pct` in stats doesn't
    /// drift down as entries expire or are LRU-evicted.
    total_hit_count: AtomicU64,
}

impl CachedProvider {
    /// Wrap an existing provider with response caching.
    pub fn new(inner: Arc<dyn LlmProvider>, config: ResponseCacheConfig) -> Self {
        Self {
            inner,
            cache: Mutex::new(HashMap::new()),
            config,
            request_count: AtomicU64::new(0),
            total_hit_count: AtomicU64::new(0),
        }
    }

    /// Number of entries currently in the cache.
    pub fn len(&self) -> usize {
        self.cache.lock().unwrap_or_else(|e| e.into_inner()).len()
    }

    /// Whether the cache is empty.
    pub fn is_empty(&self) -> bool {
        self.cache
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .is_empty()
    }

    /// Total cache hits since this provider was created.
    ///
    /// Backed by an atomic counter that is never decremented on eviction,
    /// so the value is accurate even under high eviction pressure.
    pub fn total_hits(&self) -> u64 {
        self.total_hit_count.load(Ordering::Relaxed)
    }

    /// Clear all cached entries.
    pub fn clear(&self) {
        self.cache.lock().unwrap_or_else(|e| e.into_inner()).clear();
    }

    /// Emit a cache statistics log line if `req_no` is a multiple of
    /// [`STATS_LOG_EVERY_N`]. `total_hits` must come from the `total_hit_count`
    /// atomic so it accurately reflects hits that occurred on since-evicted
    /// entries. Must be called while holding the cache lock so that
    /// `entry_count` is consistent with the snapshot.
    fn maybe_log_stats(guard: &HashMap<String, CacheEntry>, req_no: u64, total_hits: u64) {
        if req_no.is_multiple_of(STATS_LOG_EVERY_N) {
            let hit_rate = total_hits as f64 / req_no as f64 * 100.0;
            tracing::info!(
                total_requests = req_no,
                total_hits,
                hit_rate_pct = format!("{hit_rate:.1}"),
                entry_count = guard.len(),
                "LLM response cache statistics"
            );
        }
    }
}

/// Build a deterministic cache key from a completion request.
///
/// Hashes the model name, messages, and response-affecting parameters
/// (max_tokens, temperature, stop_sequences) via SHA-256. Two requests
/// with identical content and parameters produce the same key.
fn cache_key(model: &str, request: &CompletionRequest) -> String {
    let mut hasher = Sha256::new();
    hasher.update(model.as_bytes());
    hasher.update(b"|");

    // Messages are Serialize, so we can deterministically hash them.
    // serde_json produces stable output for the same input structure.
    if let Ok(json) = serde_json::to_string(&request.messages) {
        hasher.update(json.as_bytes());
    }

    // Include response-affecting parameters so different temperatures,
    // max_tokens, or stop sequences produce distinct cache keys.
    hasher.update(b"|");
    if let Some(max_tokens) = request.max_tokens {
        hasher.update(max_tokens.to_le_bytes());
    }
    hasher.update(b"|");
    if let Some(temp) = request.temperature {
        hasher.update(temp.to_le_bytes());
    }
    hasher.update(b"|");
    if let Some(ref stops) = request.stop_sequences {
        for s in stops {
            hasher.update(s.as_bytes());
            hasher.update(b"\x00");
        }
    }

    format!("{:x}", hasher.finalize())
}

impl crate::llm::NativeLlmProvider for CachedProvider {
    fn model_name(&self) -> &str {
        self.inner.model_name()
    }

    fn cost_per_token(&self) -> (Decimal, Decimal) {
        self.inner.cost_per_token()
    }

    fn cache_write_multiplier(&self) -> Decimal {
        self.inner.cache_write_multiplier()
    }

    fn cache_read_discount(&self) -> Decimal {
        self.inner.cache_read_discount()
    }

    async fn complete(&self, request: CompletionRequest) -> Result<CompletionResponse, LlmError> {
        let effective_model = self.inner.effective_model_name(request.model.as_deref());
        let key = cache_key(&effective_model, &request);
        let now = Instant::now();
        let req_no = self.request_count.fetch_add(1, Ordering::Relaxed) + 1;

        // Check cache — lock not held across the .await below.
        {
            let mut guard = self.cache.lock().unwrap_or_else(|e| e.into_inner());
            if let Some(entry) = guard.get_mut(&key) {
                if now.duration_since(entry.created_at) < self.config.ttl {
                    entry.last_accessed = now;
                    entry.hit_count += 1;
                    let hit_count = entry.hit_count;
                    // Clone now so we can release the mutable borrow before stats.
                    let cached_response = entry.response.clone();
                    tracing::trace!(hits = hit_count, "response cache hit");
                    // Drop the mutable borrow of `entry` before reading `guard` immutably.
                    let _ = entry;
                    let total_hits = self.total_hit_count.fetch_add(1, Ordering::Relaxed) + 1;
                    Self::maybe_log_stats(&guard, req_no, total_hits);
                    return Ok(cached_response);
                }
                // Expired, remove it
                guard.remove(&key);
            }
        }

        // Cache miss — call the real provider.
        let result = self.inner.complete(request).await;

        // Store result and maybe log stats, all within one lock acquisition.
        // Stats are logged even on provider error so milestone intervals are
        // not silently skipped.
        {
            let mut guard = self.cache.lock().unwrap_or_else(|e| e.into_inner());
            let total_hits = self.total_hit_count.load(Ordering::Relaxed);

            let response = match result {
                Err(e) => {
                    Self::maybe_log_stats(&guard, req_no, total_hits);
                    return Err(e);
                }
                Ok(r) => r,
            };

            // Evict expired entries
            guard.retain(|_, entry| now.duration_since(entry.created_at) < self.config.ttl);

            // LRU eviction if over capacity
            while guard.len() >= self.config.max_entries {
                let oldest_key = guard
                    .iter()
                    .min_by_key(|(_, entry)| entry.last_accessed)
                    .map(|(k, _)| k.clone());

                if let Some(k) = oldest_key {
                    guard.remove(&k);
                } else {
                    break;
                }
            }

            guard.insert(
                key,
                CacheEntry {
                    response: response.clone(),
                    created_at: now,
                    last_accessed: now,
                    hit_count: 0,
                },
            );

            Self::maybe_log_stats(&guard, req_no, total_hits);
            Ok(response)
        }
    }

    async fn complete_with_tools(
        &self,
        request: ToolCompletionRequest,
    ) -> Result<ToolCompletionResponse, LlmError> {
        // Never cache tool calls; they can trigger side effects.
        self.inner.complete_with_tools(request).await
    }

    async fn list_models(&self) -> Result<Vec<String>, LlmError> {
        self.inner.list_models().await
    }

    async fn model_metadata(&self) -> Result<ModelMetadata, LlmError> {
        self.inner.model_metadata().await
    }

    fn effective_model_name(&self, requested_model: Option<&str>) -> String {
        self.inner.effective_model_name(requested_model)
    }

    fn active_model_name(&self) -> String {
        self.inner.active_model_name()
    }

    fn set_model(&self, model: &str) -> Result<(), LlmError> {
        // Cache keys embed the active model name via `effective_model_name()`, so
        // requests to the new model automatically land in a separate cache slot.
        // Entries for the old model remain valid: if we switch back, they will be
        // hit again rather than wasted. Natural TTL / LRU eviction cleans them up.
        self.inner.set_model(model)
    }

    fn calculate_cost(&self, input_tokens: u32, output_tokens: u32) -> Decimal {
        self.inner.calculate_cost(input_tokens, output_tokens)
    }
}

#[cfg(test)]
mod tests;
