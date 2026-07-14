//! State machine transition, error classification, and delegation tests
//! for the circuit breaker.

use std::sync::Arc;
use std::time::Duration;

use rust_decimal::Decimal;

use crate::llm::circuit_breaker::{
    CircuitBreakerConfig, CircuitBreakerProvider, CircuitState, is_transient,
};
use crate::llm::error::LlmError;
use crate::llm::provider::LlmProvider;
use crate::testing::StubLlm;

use super::{fast_config, make_request, make_tool_request};

// -- State machine tests --

#[tokio::test]
async fn closed_allows_calls_and_resets_on_success() {
    let stub = Arc::new(StubLlm::new("ok").with_model_name("test"));
    let cb = CircuitBreakerProvider::new(stub, fast_config(3));

    let resp = cb.complete(make_request()).await;
    assert!(resp.is_ok());
    assert_eq!(cb.circuit_state().await, CircuitState::Closed);
    assert_eq!(cb.consecutive_failures().await, 0);
}

#[tokio::test]
async fn failures_accumulate_then_trip_to_open() {
    let stub = Arc::new(StubLlm::failing("test"));
    let cb = CircuitBreakerProvider::new(stub, fast_config(3));

    // First 2 failures: still closed
    for i in 0..2 {
        let _ = cb.complete(make_request()).await;
        assert_eq!(cb.circuit_state().await, CircuitState::Closed);
        assert_eq!(cb.consecutive_failures().await, i + 1);
    }

    // 3rd failure: trips to open
    let _ = cb.complete(make_request()).await;
    assert_eq!(cb.circuit_state().await, CircuitState::Open);
}

#[tokio::test]
async fn open_rejects_immediately() {
    let stub = Arc::new(StubLlm::failing("test"));
    let cb = CircuitBreakerProvider::new(
        stub,
        CircuitBreakerConfig {
            failure_threshold: 1,
            recovery_timeout: Duration::from_secs(60),
            half_open_successes_needed: 1,
        },
    );

    // Trip the breaker
    let _ = cb.complete(make_request()).await;
    assert_eq!(cb.circuit_state().await, CircuitState::Open);

    // Next call should fail with circuit breaker message
    let err = cb.complete(make_request()).await.unwrap_err();
    match err {
        LlmError::RequestFailed { reason, .. } => {
            assert!(
                reason.contains("Circuit breaker open"),
                "Expected circuit breaker message, got: {}",
                reason
            );
        }
        other => panic!("Expected RequestFailed, got: {:?}", other),
    }
}

#[tokio::test]
async fn recovery_timeout_transitions_to_half_open() {
    let stub = Arc::new(StubLlm::failing("test"));
    let cb = CircuitBreakerProvider::new(stub, fast_config(1));

    // Trip to open
    let _ = cb.complete(make_request()).await;
    assert_eq!(cb.circuit_state().await, CircuitState::Open);

    // Wait for recovery timeout
    tokio::time::sleep(Duration::from_millis(60)).await;

    // Next call should transition to half-open (and fail, since stub fails)
    let _ = cb.complete(make_request()).await;
    // Failed probe sends it back to Open
    assert_eq!(cb.circuit_state().await, CircuitState::Open);
}

#[tokio::test]
async fn half_open_success_closes_circuit() {
    let stub = Arc::new(StubLlm::failing("test"));
    let cb = CircuitBreakerProvider::new(stub.clone(), fast_config(1));

    // Trip to open
    let _ = cb.complete(make_request()).await;
    assert_eq!(cb.circuit_state().await, CircuitState::Open);

    // Wait for recovery, then make the stub succeed
    tokio::time::sleep(Duration::from_millis(60)).await;
    stub.set_failing(false);

    // Probe should succeed, closing the circuit
    let resp = cb.complete(make_request()).await;
    assert!(resp.is_ok());
    assert_eq!(cb.circuit_state().await, CircuitState::Closed);
    assert_eq!(cb.consecutive_failures().await, 0);
}

#[tokio::test]
async fn half_open_failure_reopens_circuit() {
    let stub = Arc::new(StubLlm::failing("test"));
    let cb = CircuitBreakerProvider::new(stub, fast_config(1));

    // Trip to open
    let _ = cb.complete(make_request()).await;

    // Wait for recovery timeout
    tokio::time::sleep(Duration::from_millis(60)).await;

    // Probe fails (stub still failing)
    let _ = cb.complete(make_request()).await;
    assert_eq!(cb.circuit_state().await, CircuitState::Open);
}

#[tokio::test]
async fn non_transient_errors_do_not_trip_breaker() {
    let stub = Arc::new(StubLlm::failing_non_transient("test"));
    let cb = CircuitBreakerProvider::new(stub, fast_config(1));

    // ContextLengthExceeded is not transient; breaker should stay closed
    for _ in 0..5 {
        let _ = cb.complete(make_request()).await;
    }
    assert_eq!(cb.circuit_state().await, CircuitState::Closed);
    assert_eq!(cb.consecutive_failures().await, 0);
}

#[tokio::test]
async fn success_resets_failure_count() {
    let stub = Arc::new(StubLlm::failing("test"));
    let cb = CircuitBreakerProvider::new(stub.clone(), fast_config(3));

    // Accumulate 2 failures
    let _ = cb.complete(make_request()).await;
    let _ = cb.complete(make_request()).await;
    assert_eq!(cb.consecutive_failures().await, 2);

    // One success resets the counter
    stub.set_failing(false);
    let resp = cb.complete(make_request()).await;
    assert!(resp.is_ok());
    assert_eq!(cb.consecutive_failures().await, 0);
}

#[tokio::test]
async fn complete_with_tools_uses_same_breaker_logic() {
    let stub = Arc::new(StubLlm::failing("test"));
    let cb = CircuitBreakerProvider::new(stub, fast_config(2));

    let _ = cb.complete_with_tools(make_tool_request()).await;
    let _ = cb.complete_with_tools(make_tool_request()).await;
    assert_eq!(cb.circuit_state().await, CircuitState::Open);
}

#[tokio::test]
async fn multiple_half_open_successes_needed() {
    let stub = Arc::new(StubLlm::failing("test"));
    let cb = CircuitBreakerProvider::new(
        stub.clone(),
        CircuitBreakerConfig {
            failure_threshold: 1,
            recovery_timeout: Duration::from_millis(50),
            half_open_successes_needed: 3,
        },
    );

    // Trip to open
    let _ = cb.complete(make_request()).await;

    // Wait and flip to succeed
    tokio::time::sleep(Duration::from_millis(60)).await;
    stub.set_failing(false);

    // First probe: half-open, success but not enough yet
    let _ = cb.complete(make_request()).await;
    assert_eq!(cb.circuit_state().await, CircuitState::HalfOpen);

    // Second probe: still half-open
    let _ = cb.complete(make_request()).await;
    assert_eq!(cb.circuit_state().await, CircuitState::HalfOpen);

    // Third probe: closes
    let _ = cb.complete(make_request()).await;
    assert_eq!(cb.circuit_state().await, CircuitState::Closed);
}

// -- Error classification tests --

#[test]
fn transient_classification() {
    // Transient
    assert!(is_transient(&LlmError::RequestFailed {
        provider: "p".into(),
        reason: "err".into(),
    }));
    assert!(is_transient(&LlmError::RateLimited {
        provider: "p".into(),
        retry_after: None,
    }));
    assert!(is_transient(&LlmError::InvalidResponse {
        provider: "p".into(),
        reason: "bad".into(),
    }));
    assert!(is_transient(&LlmError::SessionExpired {
        provider: "p".into(),
    }));
    assert!(is_transient(&LlmError::SessionRenewalFailed {
        provider: "p".into(),
        reason: "timeout".into(),
    }));
    assert!(is_transient(&LlmError::Io(std::io::Error::new(
        std::io::ErrorKind::ConnectionReset,
        "reset"
    ))));

    // NOT transient
    assert!(!is_transient(&LlmError::AuthFailed {
        provider: "p".into(),
    }));
    assert!(!is_transient(&LlmError::ContextLengthExceeded {
        used: 100_000,
        limit: 50_000,
    }));
    assert!(!is_transient(&LlmError::ModelNotAvailable {
        provider: "p".into(),
        model: "m".into(),
    }));
    assert!(!is_transient(&LlmError::Json(
        serde_json::from_str::<String>("bad").unwrap_err()
    )));
}

// -- Passthrough delegation tests --

#[tokio::test]
async fn passthrough_methods_delegate_to_inner() {
    let stub = Arc::new(StubLlm::new("ok").with_model_name("my-model"));
    let cb = CircuitBreakerProvider::new(stub, fast_config(3));

    assert_eq!(cb.model_name(), "my-model");
    assert_eq!(cb.active_model_name(), "my-model");
    assert_eq!(cb.cost_per_token(), (Decimal::ZERO, Decimal::ZERO));
    assert_eq!(cb.calculate_cost(100, 50), Decimal::ZERO);
}
