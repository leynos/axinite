//! Chaos tests for the failover chain: cascades, cooldowns, non-transient
//! error propagation, and full retry + failover + circuit breaker stacks.

use std::sync::Arc;
use std::time::Duration;

use rust_decimal::Decimal;

use ironclaw::error::LlmError;
use ironclaw::llm::{
    CircuitBreakerConfig, CircuitBreakerProvider, CompletionRequest, CompletionResponse,
    CooldownConfig, FailoverProvider, LlmProvider, RetryConfig, RetryProvider,
    ToolCompletionRequest, ToolCompletionResponse,
};

use super::providers::{FlakeyProvider, GarbageProvider, ReliableProvider, make_request};

// ---------------------------------------------------------------------------
// Test: Failover chain under chaos
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_failover_chain_under_chaos() {
    // First provider is flakey (fails 3 times), second is reliable.
    // FailoverProvider should fall back to the reliable one on failures
    // from the flakey provider, then route back to flakey once it recovers.
    //
    // Use a high cooldown threshold (100) so the flakey provider doesn't
    // enter cooldown during this test -- we want to test pure failover
    // behaviour, not cooldown.
    let flakey: Arc<dyn LlmProvider> =
        Arc::new(FlakeyProvider::new(3, "flakey recovered").with_name("flakey-primary"));
    let reliable: Arc<dyn LlmProvider> =
        Arc::new(ReliableProvider::new("reliable-backup", "backup response"));

    let config = CooldownConfig {
        cooldown_duration: Duration::from_secs(300),
        failure_threshold: 100, // high threshold: no cooldown during this test
    };
    let failover = FailoverProvider::with_cooldown(vec![flakey.clone(), reliable.clone()], config)
        .expect("should create failover with 2 providers");

    // Request 1: flakey fails, reliable succeeds.
    let r = failover.complete(make_request()).await.unwrap();
    assert_eq!(r.content, "backup response");

    // Request 2: flakey fails again, reliable succeeds.
    let r = failover.complete(make_request()).await.unwrap();
    assert_eq!(r.content, "backup response");

    // Request 3: flakey fails (third failure), reliable succeeds.
    let r = failover.complete(make_request()).await.unwrap();
    assert_eq!(r.content, "backup response");

    // Request 4: flakey should now succeed (all 3 failures consumed).
    let r = failover.complete(make_request()).await.unwrap();
    assert_eq!(r.content, "flakey recovered");
}

/// Failover with cooldown: flakey provider enters cooldown, backup serves,
/// then flakey recovers after cooldown expires.
#[tokio::test]
async fn test_failover_cooldown_with_flakey_provider() {
    let flakey: Arc<dyn LlmProvider> =
        Arc::new(FlakeyProvider::new(3, "flakey back").with_name("flakey-cd"));
    let reliable: Arc<dyn LlmProvider> = Arc::new(ReliableProvider::new("reliable-cd", "reliable"));

    let config = CooldownConfig {
        cooldown_duration: Duration::from_millis(50),
        failure_threshold: 2,
    };
    let failover = FailoverProvider::with_cooldown(vec![flakey.clone(), reliable.clone()], config)
        .expect("should create failover with cooldown");

    // Requests 1-2: flakey fails twice, reaching cooldown threshold.
    let r = failover.complete(make_request()).await.unwrap();
    assert_eq!(r.content, "reliable");
    let r = failover.complete(make_request()).await.unwrap();
    assert_eq!(r.content, "reliable");

    // Request 3: flakey should be in cooldown, only reliable called.
    // (flakey's 3rd failure would be consumed if called, but it's skipped.)
    let r = failover.complete(make_request()).await.unwrap();
    assert_eq!(r.content, "reliable");

    // Wait for cooldown to expire, then flakey gets retried.
    tokio::time::sleep(Duration::from_millis(60)).await;

    // After cooldown: flakey is tried again. It still has 1 failure remaining.
    let r = failover.complete(make_request()).await.unwrap();
    // Flakey fails again (3rd failure consumed), reliable serves.
    assert_eq!(r.content, "reliable");

    // Wait again for cooldown.
    tokio::time::sleep(Duration::from_millis(60)).await;

    // Now flakey should succeed (all 3 failures consumed).
    let r = failover.complete(make_request()).await.unwrap();
    assert_eq!(r.content, "flakey back");
}

/// Three providers: first always fails, second is flakey, third is reliable.
/// Tests cascading failover through multiple providers.
#[tokio::test]
async fn test_failover_three_provider_cascade() {
    let always_fail: Arc<dyn LlmProvider> =
        Arc::new(FlakeyProvider::new(u32::MAX, "unreachable").with_name("always-fail"));
    let flakey: Arc<dyn LlmProvider> =
        Arc::new(FlakeyProvider::new(2, "flakey ok").with_name("flakey-middle"));
    let reliable: Arc<dyn LlmProvider> =
        Arc::new(ReliableProvider::new("reliable-last", "last resort"));

    let failover = FailoverProvider::new(vec![always_fail, flakey.clone(), reliable.clone()])
        .expect("three providers");

    // Request 1: always-fail fails, flakey fails (1st), reliable serves.
    let r = failover.complete(make_request()).await.unwrap();
    assert_eq!(r.content, "last resort");

    // Request 2: always-fail fails, flakey fails (2nd), reliable serves.
    let r = failover.complete(make_request()).await.unwrap();
    assert_eq!(r.content, "last resort");

    // Request 3: always-fail fails, flakey now succeeds.
    let r = failover.complete(make_request()).await.unwrap();
    assert_eq!(r.content, "flakey ok");
}

/// Failover with a mix of transient and non-transient errors.
/// Non-transient error from primary should propagate immediately.
#[tokio::test]
async fn test_failover_non_transient_stops_chain() {
    // Provider that returns a non-transient error.
    struct NonTransientProvider;

    impl ironclaw::llm::NativeLlmProvider for NonTransientProvider {
        fn model_name(&self) -> &str {
            "non-transient"
        }
        fn cost_per_token(&self) -> (Decimal, Decimal) {
            (Decimal::ZERO, Decimal::ZERO)
        }
        async fn complete(
            &self,
            _request: CompletionRequest,
        ) -> Result<CompletionResponse, LlmError> {
            Err(LlmError::ContextLengthExceeded {
                used: 200_000,
                limit: 100_000,
            })
        }
        async fn complete_with_tools(
            &self,
            _request: ToolCompletionRequest,
        ) -> Result<ToolCompletionResponse, LlmError> {
            Err(LlmError::ContextLengthExceeded {
                used: 200_000,
                limit: 100_000,
            })
        }
    }

    let primary: Arc<dyn LlmProvider> = Arc::new(NonTransientProvider);
    let backup = Arc::new(ReliableProvider::new("backup", "should not reach"));

    let failover = FailoverProvider::new(vec![primary, backup.clone() as Arc<dyn LlmProvider>])
        .expect("failover");

    let err = failover.complete(make_request()).await.unwrap_err();
    assert!(
        matches!(err, LlmError::ContextLengthExceeded { .. }),
        "non-transient error should propagate: {:?}",
        err
    );
    // Backup should never have been called.
    assert_eq!(
        backup.calls(),
        0,
        "backup should not be called for non-transient errors"
    );
}

/// Full stack: RetryProvider wrapping FlakeyProvider, behind a
/// CircuitBreakerProvider. Verifies the full chain works together.
#[tokio::test]
async fn test_retry_plus_circuit_breaker_integration() {
    // Flakey provider that fails 2 times then succeeds.
    let flakey = Arc::new(FlakeyProvider::new(2, "stack success"));
    let retry: Arc<dyn LlmProvider> = Arc::new(RetryProvider::new(
        flakey.clone(),
        RetryConfig { max_retries: 3 },
    ));
    let cb = CircuitBreakerProvider::new(
        retry,
        CircuitBreakerConfig {
            failure_threshold: 10, // high threshold so we don't trip
            recovery_timeout: Duration::from_secs(30),
            half_open_successes_needed: 1,
        },
    );

    let result = tokio::time::timeout(Duration::from_secs(30), cb.complete(make_request()))
        .await
        .expect("should not timeout");

    let response = result.expect("retry+CB stack should succeed");
    assert_eq!(response.content, "stack success");
    assert_eq!(
        cb.circuit_state().await,
        ironclaw::llm::circuit_breaker::CircuitState::Closed,
        "circuit should remain closed"
    );
}

/// Full chain: RetryProvider -> FailoverProvider -> CircuitBreakerProvider.
/// Primary is flakey with insufficient retries to recover; failover catches it.
#[tokio::test]
async fn test_full_chain_retry_failover_circuit_breaker() {
    // Primary: flakey, fails 5 times. Retry allows 2 retries (3 attempts).
    // After retry exhaustion, failover should kick in to the reliable backup.
    let flakey = Arc::new(FlakeyProvider::new(5, "not reachable").with_name("flakey-full"));
    let retry_primary: Arc<dyn LlmProvider> = Arc::new(RetryProvider::new(
        flakey.clone(),
        RetryConfig { max_retries: 2 },
    ));

    // Backup: always reliable.
    let reliable: Arc<dyn LlmProvider> =
        Arc::new(ReliableProvider::new("reliable-full", "backup ok"));

    // Failover wraps both.
    let failover: Arc<dyn LlmProvider> =
        Arc::new(FailoverProvider::new(vec![retry_primary, reliable.clone()]).expect("failover"));

    // Circuit breaker on top.
    let cb = CircuitBreakerProvider::new(
        failover,
        CircuitBreakerConfig {
            failure_threshold: 10,
            recovery_timeout: Duration::from_secs(30),
            half_open_successes_needed: 1,
        },
    );

    let result = tokio::time::timeout(Duration::from_secs(30), cb.complete(make_request()))
        .await
        .expect("should not timeout");

    let response = result.expect("full chain should succeed via failover");
    assert_eq!(response.content, "backup ok");
}

/// Verify that GarbageProvider content flows through the full decorator chain
/// without causing panics or unexpected errors.
#[tokio::test]
async fn test_garbage_through_full_chain() {
    let garbage: Arc<dyn LlmProvider> = Arc::new(GarbageProvider::new("garbage-chain"));
    let retry: Arc<dyn LlmProvider> = Arc::new(RetryProvider::new(
        garbage.clone(),
        RetryConfig { max_retries: 1 },
    ));
    let cb = CircuitBreakerProvider::new(
        retry,
        CircuitBreakerConfig {
            failure_threshold: 5,
            recovery_timeout: Duration::from_secs(30),
            half_open_successes_needed: 1,
        },
    );

    let result = cb.complete(make_request()).await;
    assert!(result.is_ok(), "garbage should flow through without error");

    let response = result.unwrap();
    assert!(
        response.content.contains("garbage"),
        "garbage content should be preserved"
    );
    assert_eq!(
        cb.circuit_state().await,
        ironclaw::llm::circuit_breaker::CircuitState::Closed,
        "Ok responses should not trip the breaker"
    );
}
