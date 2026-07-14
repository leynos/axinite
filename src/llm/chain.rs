//! Provider chain assembly for the LLM subsystem.
//!
//! Wraps the raw provider with retry, smart routing, failover, circuit
//! breaker, response cache, and recording decorators in a fixed order.

use std::sync::Arc;

use super::circuit_breaker::{CircuitBreakerConfig, CircuitBreakerProvider};
use super::config::LlmConfig;
use super::error::LlmError;
use super::factory::{
    create_cheap_llm_provider, create_llm_provider, create_llm_provider_with_config,
};
use super::failover::{CooldownConfig, FailoverProvider};
use super::provider::LlmProvider;
use super::recording::RecordingLlm;
use super::response_cache::{CachedProvider, ResponseCacheConfig};
use super::retry::{RetryConfig, RetryProvider};
use super::session::SessionManager;
use super::smart_routing::{SmartRoutingConfig, SmartRoutingProvider};

/// Build the full LLM provider chain with all configured wrappers.
///
/// Applies decorators in this order:
/// 1. Raw provider (from config)
/// 2. RetryProvider (per-provider retry with exponential backoff)
/// 3. SmartRoutingProvider (cheap/primary split when cheap model is configured)
/// 4. FailoverProvider (fallback model when primary fails)
/// 5. CircuitBreakerProvider (fast-fail when backend is degraded)
/// 6. CachedProvider (in-memory response cache)
///
/// Also returns a separate cheap LLM provider for heartbeat/evaluation (not
/// part of the chain — it's a standalone provider for explicitly cheap tasks).
///
/// This is the single source of truth for provider chain construction,
/// called by both `main.rs` and `app.rs`.
#[allow(clippy::type_complexity)]
pub async fn build_provider_chain(
    config: &LlmConfig,
    session: Arc<SessionManager>,
) -> Result<
    (
        Arc<dyn LlmProvider>,
        Option<Arc<dyn LlmProvider>>,
        Option<Arc<RecordingLlm>>,
    ),
    LlmError,
> {
    let llm = create_llm_provider(config, session.clone()).await?;
    tracing::debug!("LLM provider initialized: {}", llm.model_name());

    let retry_config = RetryConfig {
        max_retries: config.nearai.max_retries,
    };

    // 1. Retry
    let llm = wrap_retry_logged(llm, &retry_config);
    // 2. Smart routing (cheap/primary split)
    let llm = wrap_smart_routing(llm, config, &session, &retry_config)?;
    // 3. Failover
    let llm = wrap_failover(llm, config, &session, &retry_config)?;
    // 4. Circuit breaker
    let llm = wrap_circuit_breaker(llm, config);
    // 5. Response cache
    let llm = wrap_response_cache(llm, config);

    // 6. Recording (trace capture for replay testing)
    let recording_handle = RecordingLlm::from_env(llm.clone());
    let llm: Arc<dyn LlmProvider> = if let Some(ref recorder) = recording_handle {
        Arc::clone(recorder) as Arc<dyn LlmProvider>
    } else {
        llm
    };

    // Standalone cheap LLM for heartbeat/evaluation (not part of the chain)
    let cheap_llm = create_cheap_llm_provider(config, session)?;
    if let Some(ref cheap) = cheap_llm {
        tracing::debug!("Cheap LLM provider initialized: {}", cheap.model_name());
    }

    Ok((llm, cheap_llm, recording_handle))
}

/// Wrap a provider with retries when retries are configured (silent).
fn wrap_retry(llm: Arc<dyn LlmProvider>, retry_config: &RetryConfig) -> Arc<dyn LlmProvider> {
    if retry_config.max_retries > 0 {
        Arc::new(RetryProvider::new(llm, retry_config.clone()))
    } else {
        llm
    }
}

/// Wrap the primary provider with retries, logging when enabled.
fn wrap_retry_logged(
    llm: Arc<dyn LlmProvider>,
    retry_config: &RetryConfig,
) -> Arc<dyn LlmProvider> {
    if retry_config.max_retries > 0 {
        tracing::debug!(
            max_retries = retry_config.max_retries,
            "LLM retry wrapper enabled"
        );
    }
    wrap_retry(llm, retry_config)
}

/// Add the cheap/primary smart-routing split when a cheap model is
/// configured.
fn wrap_smart_routing(
    llm: Arc<dyn LlmProvider>,
    config: &LlmConfig,
    session: &Arc<SessionManager>,
    retry_config: &RetryConfig,
) -> Result<Arc<dyn LlmProvider>, LlmError> {
    let Some(ref cheap_model) = config.nearai.cheap_model else {
        return Ok(llm);
    };
    let mut cheap_config = config.nearai.clone();
    cheap_config.model = cheap_model.clone();
    let cheap = create_llm_provider_with_config(
        &cheap_config,
        session.clone(),
        config.request_timeout_secs,
    )?;
    let cheap = wrap_retry(cheap, retry_config);
    tracing::debug!(
        primary = %llm.model_name(),
        cheap = %cheap.model_name(),
        "Smart routing enabled"
    );
    Ok(Arc::new(SmartRoutingProvider::new(
        llm,
        cheap,
        SmartRoutingConfig {
            cascade_enabled: config.nearai.smart_routing_cascade,
            ..SmartRoutingConfig::default()
        },
    )))
}

/// Add the fallback-model failover wrapper when configured.
fn wrap_failover(
    llm: Arc<dyn LlmProvider>,
    config: &LlmConfig,
    session: &Arc<SessionManager>,
    retry_config: &RetryConfig,
) -> Result<Arc<dyn LlmProvider>, LlmError> {
    let Some(ref fallback_model) = config.nearai.fallback_model else {
        return Ok(llm);
    };
    if fallback_model == &config.nearai.model {
        tracing::warn!(
            "fallback_model is the same as primary model, failover may not be effective"
        );
    }
    let mut fallback_config = config.nearai.clone();
    fallback_config.model = fallback_model.clone();
    let fallback = create_llm_provider_with_config(
        &fallback_config,
        session.clone(),
        config.request_timeout_secs,
    )?;
    tracing::debug!(
        primary = %llm.model_name(),
        fallback = %fallback.model_name(),
        "LLM failover enabled"
    );
    let fallback = wrap_retry(fallback, retry_config);
    let cooldown_config = CooldownConfig {
        cooldown_duration: std::time::Duration::from_secs(config.nearai.failover_cooldown_secs),
        failure_threshold: config.nearai.failover_cooldown_threshold,
    };
    Ok(Arc::new(FailoverProvider::with_cooldown(
        vec![llm, fallback],
        cooldown_config,
    )?))
}

/// Add the circuit breaker when a failure threshold is configured.
fn wrap_circuit_breaker(llm: Arc<dyn LlmProvider>, config: &LlmConfig) -> Arc<dyn LlmProvider> {
    let Some(threshold) = config.nearai.circuit_breaker_threshold else {
        return llm;
    };
    let cb_config = CircuitBreakerConfig {
        failure_threshold: threshold,
        recovery_timeout: std::time::Duration::from_secs(
            config.nearai.circuit_breaker_recovery_secs,
        ),
        ..CircuitBreakerConfig::default()
    };
    tracing::debug!(
        threshold,
        recovery_secs = config.nearai.circuit_breaker_recovery_secs,
        "LLM circuit breaker enabled"
    );
    Arc::new(CircuitBreakerProvider::new(llm, cb_config))
}

/// Add the in-memory response cache when enabled.
fn wrap_response_cache(llm: Arc<dyn LlmProvider>, config: &LlmConfig) -> Arc<dyn LlmProvider> {
    if !config.nearai.response_cache_enabled {
        return llm;
    }
    let rc_config = ResponseCacheConfig {
        ttl: std::time::Duration::from_secs(config.nearai.response_cache_ttl_secs),
        max_entries: config.nearai.response_cache_max_entries,
    };
    tracing::debug!(
        ttl_secs = config.nearai.response_cache_ttl_secs,
        max_entries = config.nearai.response_cache_max_entries,
        "LLM response cache enabled"
    );
    Arc::new(CachedProvider::new(llm, rc_config))
}
