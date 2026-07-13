//! Multi-provider LLM failover.
//!
//! Wraps multiple LlmProvider instances and tries each in sequence
//! until one succeeds. Transparent to callers --- same LlmProvider trait.
//!
//! Providers that fail repeatedly are temporarily placed in cooldown
//! so subsequent requests skip them, reducing latency when a provider
//! is known to be down. Cooldown state is lock-free (atomics only).

use std::collections::HashMap;
use std::future::Future;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Instant;

use rust_decimal::Decimal;

use crate::llm::error::LlmError;
use crate::llm::provider::{
    CompletionRequest, CompletionResponse, LlmProvider, ModelMetadata, ToolCompletionRequest,
    ToolCompletionResponse,
};

use crate::llm::retry::is_retryable;

mod cooldown;

pub use cooldown::CooldownConfig;
use cooldown::ProviderCooldown;

#[cfg(test)]
mod tests;

/// An LLM provider that wraps multiple providers and tries each in sequence
/// on transient failures.
///
/// The first provider in the list is the primary. If it fails with a retryable
/// error, the next provider is tried, and so on. Non-retryable errors
/// (e.g. `AuthFailed`, `ContextLengthExceeded`) propagate immediately.
///
/// Providers that repeatedly fail with retryable errors are temporarily
/// placed in cooldown and skipped, reducing latency.
pub struct FailoverProvider {
    providers: Vec<Arc<dyn LlmProvider>>,
    /// Index of the provider that last handled a request successfully.
    /// Used by `model_name()` and `cost_per_token()` so downstream cost
    /// tracking reflects the provider that actually served the request.
    last_used: AtomicUsize,
    /// Per-provider cooldown tracking (same length as `providers`).
    cooldowns: Vec<ProviderCooldown>,
    /// Reference instant for computing elapsed nanos. Shared across all
    /// cooldown timestamps so they are comparable.
    epoch: Instant,
    /// Cooldown configuration.
    cooldown_config: CooldownConfig,
    /// Request-scoped provider index keyed by Tokio task ID.
    ///
    /// This allows `effective_model_name()` to report the provider that handled
    /// the *current* request, even when other concurrent requests update
    /// `last_used`.
    provider_for_task: Mutex<HashMap<tokio::task::Id, usize>>,
}

impl FailoverProvider {
    /// Create a new failover provider with default cooldown settings.
    ///
    /// Returns an error if `providers` is empty.
    pub fn new(providers: Vec<Arc<dyn LlmProvider>>) -> Result<Self, LlmError> {
        Self::with_cooldown(providers, CooldownConfig::default())
    }

    /// Create a new failover provider with explicit cooldown configuration.
    ///
    /// Returns an error if `providers` is empty.
    pub fn with_cooldown(
        providers: Vec<Arc<dyn LlmProvider>>,
        cooldown_config: CooldownConfig,
    ) -> Result<Self, LlmError> {
        if providers.is_empty() {
            return Err(LlmError::RequestFailed {
                provider: "failover".to_string(),
                reason: "FailoverProvider requires at least one provider".to_string(),
            });
        }
        let cooldowns = (0..providers.len())
            .map(|_| ProviderCooldown::new())
            .collect();
        Ok(Self {
            providers,
            last_used: AtomicUsize::new(0),
            cooldowns,
            epoch: Instant::now(),
            cooldown_config,
            provider_for_task: Mutex::new(HashMap::new()),
        })
    }

    /// Nanoseconds elapsed since `self.epoch`.
    ///
    /// Truncates `u128` → `u64` (wraps after ~584 years of continuous
    /// uptime). Acceptable because `epoch` is set at construction time.
    fn now_nanos(&self) -> u64 {
        self.epoch.elapsed().as_nanos() as u64
    }

    /// Current Tokio task ID if available.
    fn current_task_id() -> Option<tokio::task::Id> {
        tokio::task::try_id()
    }

    /// Bind the selected provider index to the current task.
    fn bind_provider_to_current_task(&self, provider_idx: usize) {
        let Some(task_id) = Self::current_task_id() else {
            return;
        };
        if let Ok(mut guard) = self.provider_for_task.lock() {
            guard.insert(task_id, provider_idx);
        }
    }

    /// Take and remove the provider index bound to the current task.
    fn take_bound_provider_for_current_task(&self) -> Option<usize> {
        let task_id = Self::current_task_id()?;
        self.provider_for_task
            .lock()
            .ok()
            .and_then(|mut guard| guard.remove(&task_id))
    }

    /// Try each provider in sequence until one succeeds or all fail.
    ///
    /// Providers in cooldown are skipped unless *all* providers are in
    /// cooldown, in which case the one with the oldest cooldown timestamp
    /// (most likely to have recovered) is tried.
    async fn try_providers<T, F, Fut>(&self, mut call: F) -> Result<(usize, T), LlmError>
    where
        F: FnMut(Arc<dyn LlmProvider>) -> Fut,
        Fut: Future<Output = Result<T, LlmError>>,
    {
        let now_nanos = self.now_nanos();
        let cooldown_nanos = self.cooldown_config.cooldown_duration.as_nanos() as u64;

        // Partition providers into available and cooled-down.
        let (mut available, cooled_down): (Vec<usize>, Vec<usize>) = (0..self.providers.len())
            .partition(|&i| !self.cooldowns[i].is_in_cooldown(now_nanos, cooldown_nanos));

        // Log skipped providers.
        for &i in &cooled_down {
            tracing::info!(
                provider = %self.providers[i].model_name(),
                "Skipping provider (in cooldown)"
            );
        }

        // Never skip ALL providers: if every provider is in cooldown, pick
        // the one with the oldest cooldown activation (most likely recovered).
        if available.is_empty() {
            let oldest = (0..self.providers.len())
                .min_by_key(|&i| {
                    self.cooldowns[i]
                        .cooldown_activated_nanos
                        .load(Ordering::Relaxed)
                })
                .ok_or_else(|| LlmError::RequestFailed {
                    provider: "failover".to_string(),
                    reason: "FailoverProvider requires at least one provider".to_string(),
                })?;
            tracing::info!(
                provider = %self.providers[oldest].model_name(),
                "All providers in cooldown, trying oldest-cooled provider"
            );
            available.push(oldest);
        }

        let mut last_error: Option<LlmError> = None;

        for (pos, &i) in available.iter().enumerate() {
            let provider = &self.providers[i];
            let result = call(Arc::clone(provider)).await;
            match result {
                Ok(response) => {
                    self.last_used.store(i, Ordering::Relaxed);
                    self.cooldowns[i].reset();
                    return Ok((i, response));
                }
                Err(err) => {
                    if !is_retryable(&err) {
                        return Err(err);
                    }

                    // Increment failure count; activate cooldown if threshold reached.
                    if self.cooldowns[i].record_failure(self.cooldown_config.failure_threshold) {
                        let nanos = self.now_nanos();
                        self.cooldowns[i].activate_cooldown(nanos);
                        tracing::warn!(
                            provider = %provider.model_name(),
                            threshold = self.cooldown_config.failure_threshold,
                            cooldown_secs = self.cooldown_config.cooldown_duration.as_secs(),
                            "Provider entered cooldown after repeated failures"
                        );
                    }

                    if pos + 1 < available.len() {
                        let next_i = available[pos + 1];
                        tracing::warn!(
                            provider = %provider.model_name(),
                            error = %err,
                            next_provider = %self.providers[next_i].model_name(),
                            "Provider failed with retryable error, trying next provider"
                        );
                    }
                    last_error = Some(err);
                }
            }
        }

        Err(last_error.unwrap_or_else(|| LlmError::RequestFailed {
            provider: "failover".to_string(),
            reason: "Invariant violated in FailoverProvider: providers were exhausted but no last_error was recorded (this branch should be unreachable; possible causes: no provider attempts were made or `available` was unexpectedly empty).".to_string(),
        }))
    }
}

impl crate::llm::NativeLlmProvider for FailoverProvider {
    fn model_name(&self) -> &str {
        self.providers[self.last_used.load(Ordering::Relaxed)].model_name()
    }

    fn cost_per_token(&self) -> (Decimal, Decimal) {
        self.providers[self.last_used.load(Ordering::Relaxed)].cost_per_token()
    }

    fn cache_write_multiplier(&self) -> Decimal {
        self.providers[self.last_used.load(Ordering::Relaxed)].cache_write_multiplier()
    }

    fn cache_read_discount(&self) -> Decimal {
        self.providers[self.last_used.load(Ordering::Relaxed)].cache_read_discount()
    }

    async fn complete(&self, request: CompletionRequest) -> Result<CompletionResponse, LlmError> {
        let (provider_idx, response) = self
            .try_providers(|provider| {
                let req = request.clone();
                async move { provider.complete(req).await }
            })
            .await?;
        self.bind_provider_to_current_task(provider_idx);
        Ok(response)
    }

    async fn complete_with_tools(
        &self,
        request: ToolCompletionRequest,
    ) -> Result<ToolCompletionResponse, LlmError> {
        let (provider_idx, response) = self
            .try_providers(|provider| {
                let req = request.clone();
                async move { provider.complete_with_tools(req).await }
            })
            .await?;
        self.bind_provider_to_current_task(provider_idx);
        Ok(response)
    }

    fn active_model_name(&self) -> String {
        self.providers[self.last_used.load(Ordering::Relaxed)].active_model_name()
    }

    fn set_model(&self, model: &str) -> Result<(), LlmError> {
        for provider in &self.providers {
            provider.set_model(model)?;
        }
        Ok(())
    }

    async fn list_models(&self) -> Result<Vec<String>, LlmError> {
        let mut all_models = Vec::new();

        for provider in &self.providers {
            match provider.list_models().await {
                Ok(models) => all_models.extend(models),
                Err(err) => {
                    tracing::warn!(
                        provider = %provider.model_name(),
                        error = %err,
                        "Failed to list models from provider, skipping"
                    );
                }
            }
        }

        all_models.sort();
        all_models.dedup();
        Ok(all_models)
    }

    async fn model_metadata(&self) -> Result<ModelMetadata, LlmError> {
        self.providers[self.last_used.load(Ordering::Relaxed)]
            .model_metadata()
            .await
    }

    fn calculate_cost(&self, input_tokens: u32, output_tokens: u32) -> Decimal {
        self.providers[self.last_used.load(Ordering::Relaxed)]
            .calculate_cost(input_tokens, output_tokens)
    }

    fn effective_model_name(&self, requested_model: Option<&str>) -> String {
        if let Some(provider_idx) = self.take_bound_provider_for_current_task() {
            return self.providers[provider_idx].effective_model_name(requested_model);
        }

        self.providers[self.last_used.load(Ordering::Relaxed)].effective_model_name(requested_model)
    }
}
