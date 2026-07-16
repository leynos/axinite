//! Provider chaos and edge-case tests for the circuit breaker
//! (hanging providers, rapid cycles, zero-duration timeouts).

use std::sync::Arc;
use std::time::Duration;

use rust_decimal::Decimal;

use crate::llm::circuit_breaker::{CircuitBreakerConfig, CircuitBreakerProvider, CircuitState};
use crate::llm::error::LlmError;
use crate::llm::provider::{
    CompletionRequest, CompletionResponse, LlmProvider, ToolCompletionRequest,
    ToolCompletionResponse,
};
use crate::testing::StubLlm;

use super::{fast_config, make_request};

// === QA Plan P2 - 4.1: Provider chaos tests ===

/// Provider that hangs forever (tests timeout handling at the caller).
struct HangingProvider;

impl crate::llm::NativeLlmProvider for HangingProvider {
    fn model_name(&self) -> &str {
        "hanging"
    }
    fn cost_per_token(&self) -> (Decimal, Decimal) {
        (Decimal::ZERO, Decimal::ZERO)
    }
    async fn complete(&self, _request: CompletionRequest) -> Result<CompletionResponse, LlmError> {
        // Hang forever
        std::future::pending().await
    }
    async fn complete_with_tools(
        &self,
        _request: ToolCompletionRequest,
    ) -> Result<ToolCompletionResponse, LlmError> {
        std::future::pending().await
    }
}

#[tokio::test]
async fn hanging_provider_behind_breaker_can_be_timed_out() {
    let hanging: Arc<dyn LlmProvider> = Arc::new(HangingProvider);
    let cb = CircuitBreakerProvider::new(hanging, fast_config(1));

    // The caller should be able to timeout the request.
    let result =
        tokio::time::timeout(Duration::from_millis(100), cb.complete(make_request())).await;

    // Should timeout, not hang forever.
    assert!(result.is_err(), "should timeout, not hang");
}

#[tokio::test]
async fn rapid_open_close_cycles_do_not_corrupt_state() {
    let stub = Arc::new(StubLlm::failing("test"));
    let cb = CircuitBreakerProvider::new(
        stub.clone(),
        CircuitBreakerConfig {
            failure_threshold: 1,
            recovery_timeout: Duration::from_millis(10),
            half_open_successes_needed: 1,
        },
    );

    // Cycle through open/half-open/open several times.
    for _ in 0..5 {
        // Trip to open.
        let _ = cb.complete(make_request()).await;
        assert_eq!(cb.circuit_state().await, CircuitState::Open);

        // Wait for recovery.
        tokio::time::sleep(Duration::from_millis(15)).await;

        // Probe fails (stub still failing) → back to Open.
        let _ = cb.complete(make_request()).await;
        assert_eq!(cb.circuit_state().await, CircuitState::Open);
    }

    // Now flip to succeeding and verify recovery still works.
    tokio::time::sleep(Duration::from_millis(15)).await;
    stub.set_failing(false);
    let result = cb.complete(make_request()).await;
    assert!(result.is_ok());
    assert_eq!(cb.circuit_state().await, CircuitState::Closed);
}

#[tokio::test]
async fn mixed_error_types_only_transient_counts() {
    // Non-transient errors should never trip the breaker, even after many attempts.
    let non_transient = Arc::new(StubLlm::failing_non_transient("test"));
    let cb_nt = CircuitBreakerProvider::new(non_transient, fast_config(3));

    // 100 non-transient errors should not trip the breaker.
    for _ in 0..100 {
        let _ = cb_nt.complete(make_request()).await;
    }
    assert_eq!(cb_nt.circuit_state().await, CircuitState::Closed);
    assert_eq!(cb_nt.consecutive_failures().await, 0);
}

// === QA Plan 2.6: Edge case tests ===

/// With a recovery_timeout of zero, the circuit should transition from
/// Open to HalfOpen immediately on the next call (the elapsed time
/// always >= Duration::ZERO). This verifies that zero-duration timeouts
/// are not treated as a special "disabled" sentinel.
#[tokio::test]
async fn test_cooldown_at_zero_nanos() {
    let stub = Arc::new(StubLlm::failing("test"));
    let cb = CircuitBreakerProvider::new(
        stub.clone(),
        CircuitBreakerConfig {
            failure_threshold: 1,
            recovery_timeout: Duration::ZERO,
            half_open_successes_needed: 1,
        },
    );

    // Trip the breaker with one failure.
    let _ = cb.complete(make_request()).await;
    assert_eq!(cb.circuit_state().await, CircuitState::Open);

    // With recovery_timeout = 0, the very next call should transition
    // from Open -> HalfOpen immediately (no sleep needed).
    // Since the stub is still failing, the probe will fail, sending
    // it back to Open. But the key assertion is that the transition
    // to HalfOpen actually happened (not stuck in Open forever).
    stub.set_failing(false);
    let result = cb.complete(make_request()).await;
    assert!(
        result.is_ok(),
        "zero recovery_timeout should allow immediate probe"
    );
    assert_eq!(
        cb.circuit_state().await,
        CircuitState::Closed,
        "successful probe after zero-timeout should close the circuit"
    );

    // Verify it also works when the probe fails: should re-open, not
    // get stuck in some intermediate state.
    stub.set_failing(true);
    // Trip again.
    let _ = cb.complete(make_request()).await;
    assert_eq!(cb.circuit_state().await, CircuitState::Open);
    // Next call: Open -> HalfOpen (zero timeout), probe fails -> Open.
    let _ = cb.complete(make_request()).await;
    assert_eq!(
        cb.circuit_state().await,
        CircuitState::Open,
        "failed probe should re-open circuit even with zero timeout"
    );
}

/// When in half-open state, a single failure should immediately
/// re-open the circuit (not close it or leave it in half-open).
/// Also verifies that any accumulated half_open_successes are reset.
#[tokio::test]
async fn test_circuit_breaker_half_open_failure_reopens() {
    let stub = Arc::new(StubLlm::failing("test"));
    let cb = CircuitBreakerProvider::new(
        stub.clone(),
        CircuitBreakerConfig {
            failure_threshold: 1,
            recovery_timeout: Duration::from_millis(20),
            half_open_successes_needed: 3, // require multiple successes
        },
    );

    // Trip the breaker.
    let _ = cb.complete(make_request()).await;
    assert_eq!(cb.circuit_state().await, CircuitState::Open);

    // Wait for recovery, then succeed once to accumulate 1 half-open success.
    tokio::time::sleep(Duration::from_millis(30)).await;
    stub.set_failing(false);
    let _ = cb.complete(make_request()).await;
    // Still in half-open (need 3 successes, got 1).
    assert_eq!(cb.circuit_state().await, CircuitState::HalfOpen);

    // Now fail: should immediately re-open, discarding the 1 accumulated success.
    stub.set_failing(true);
    let _ = cb.complete(make_request()).await;
    assert_eq!(
        cb.circuit_state().await,
        CircuitState::Open,
        "failure in half-open should immediately re-open the circuit"
    );

    // After re-opening, wait for recovery and verify that the half-open
    // success counter was reset (need 3 fresh successes, not 2).
    tokio::time::sleep(Duration::from_millis(30)).await;
    stub.set_failing(false);

    // First success: half-open, count=1.
    let _ = cb.complete(make_request()).await;
    assert_eq!(cb.circuit_state().await, CircuitState::HalfOpen);

    // Second success: half-open, count=2.
    let _ = cb.complete(make_request()).await;
    assert_eq!(cb.circuit_state().await, CircuitState::HalfOpen);

    // Third success: closes the circuit.
    let _ = cb.complete(make_request()).await;
    assert_eq!(
        cb.circuit_state().await,
        CircuitState::Closed,
        "3 fresh successes needed after re-open, not 2"
    );
    assert_eq!(cb.consecutive_failures().await, 0);
}
