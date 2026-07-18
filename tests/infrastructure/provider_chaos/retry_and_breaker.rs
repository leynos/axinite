//! Chaos tests for retry logic and the circuit breaker: flakey providers,
//! hanging providers, garbage responses, and breaker trip/recovery cycles.

use std::sync::Arc;
use std::time::Duration;

use axinite::error::LlmError;
use axinite::llm::{
    CircuitBreakerConfig, CircuitBreakerProvider, FinishReason, LlmProvider, RetryConfig,
    RetryProvider,
};

use super::providers::{
    FlakeyProvider, GarbageProvider, HangingProvider, make_request, make_tool_request,
};

// ---------------------------------------------------------------------------
// Test: FlakeyProvider eventually succeeds through RetryProvider
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_flakey_provider_eventually_succeeds() {
    // FlakeyProvider fails 3 times then succeeds.
    // RetryProvider with max_retries=5 should be enough to get through.
    let flakey = Arc::new(FlakeyProvider::new(3, "success after retries"));
    let retry = RetryProvider::new(flakey.clone(), RetryConfig { max_retries: 5 });

    let result = tokio::time::timeout(Duration::from_secs(30), retry.complete(make_request()))
        .await
        .expect("should not timeout with 30s budget");

    let response = result.expect("should succeed after retries");
    assert_eq!(response.content, "success after retries");
    // Should have been called 4 times: 3 failures + 1 success
    assert_eq!(
        flakey.calls(),
        4,
        "expected 3 failures + 1 success = 4 calls"
    );
}

/// Verify that a FlakeyProvider with more failures than retries exhausts
/// retries and returns an error.
#[tokio::test]
async fn test_flakey_provider_exhausts_retries() {
    // Fails 10 times, but retry allows only 2 retries (3 attempts total).
    let flakey = Arc::new(FlakeyProvider::new(10, "never reached"));
    let retry = RetryProvider::new(flakey.clone(), RetryConfig { max_retries: 2 });

    let result = retry.complete(make_request()).await;
    assert!(result.is_err(), "should fail when retries are exhausted");
    // 3 total attempts: initial + 2 retries
    assert_eq!(flakey.calls(), 3);
}

// ---------------------------------------------------------------------------
// Test: HangingProvider times out with tokio::time::timeout
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_hanging_provider_times_out() {
    let hanging: Arc<dyn LlmProvider> = Arc::new(HangingProvider::new("hanging-provider"));

    let result =
        tokio::time::timeout(Duration::from_millis(200), hanging.complete(make_request())).await;

    // Should be a timeout error, not hang forever.
    assert!(
        result.is_err(),
        "HangingProvider should timeout, not hang forever"
    );
}

/// HangingProvider behind a CircuitBreakerProvider can still be timed out.
#[tokio::test]
async fn test_hanging_provider_behind_circuit_breaker_times_out() {
    let hanging: Arc<dyn LlmProvider> = Arc::new(HangingProvider::new("hanging-behind-cb"));
    let cb = CircuitBreakerProvider::new(
        hanging,
        CircuitBreakerConfig {
            failure_threshold: 3,
            recovery_timeout: Duration::from_secs(30),
            half_open_successes_needed: 1,
        },
    );

    let result =
        tokio::time::timeout(Duration::from_millis(200), cb.complete(make_request())).await;

    assert!(
        result.is_err(),
        "should timeout even when wrapped in circuit breaker"
    );
}

/// complete_with_tools also hangs and can be timed out.
#[tokio::test]
async fn test_hanging_provider_complete_with_tools_times_out() {
    let hanging: Arc<dyn LlmProvider> = Arc::new(HangingProvider::new("hanging-tools"));

    let result = tokio::time::timeout(
        Duration::from_millis(200),
        hanging.complete_with_tools(make_tool_request()),
    )
    .await;

    assert!(result.is_err(), "complete_with_tools should also timeout");
}

// ---------------------------------------------------------------------------
// Test: GarbageProvider returns valid response with garbage content
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_garbage_provider_returns_error_or_empty() {
    let garbage = Arc::new(GarbageProvider::new("garbage-provider"));

    // complete() returns a valid CompletionResponse with garbage content.
    let response = garbage
        .complete(make_request())
        .await
        .expect("garbage provider should not return an error");

    // The response is structurally valid but the content is nonsensical.
    assert!(
        !response.content.is_empty(),
        "garbage content should be non-empty"
    );
    assert_eq!(
        response.finish_reason,
        FinishReason::Unknown,
        "garbage response has Unknown finish reason"
    );
    assert_eq!(response.input_tokens, 0);
    assert_eq!(response.output_tokens, 0);

    // complete_with_tools() returns empty content.
    let tool_response = garbage
        .complete_with_tools(make_tool_request())
        .await
        .expect("garbage provider tool completion should not error");

    assert_eq!(
        tool_response.content,
        Some(String::new()),
        "tool response should have empty content"
    );
    assert!(tool_response.tool_calls.is_empty());
    assert_eq!(garbage.calls(), 2, "should have recorded 2 calls total");
}

/// GarbageProvider is not retried by RetryProvider since it returns Ok.
#[tokio::test]
async fn test_garbage_provider_not_retried() {
    let garbage = Arc::new(GarbageProvider::new("garbage-no-retry"));
    let retry = RetryProvider::new(garbage.clone(), RetryConfig { max_retries: 3 });

    let response = retry.complete(make_request()).await;
    assert!(response.is_ok(), "garbage Ok response should pass through");
    assert_eq!(
        garbage.calls(),
        1,
        "should only call once -- no retry on Ok"
    );
}

// ---------------------------------------------------------------------------
// Test: Circuit breaker trips and recovers
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_circuit_breaker_trips_and_recovers() {
    // Use a FlakeyProvider that fails 5 times then succeeds.
    let flakey = Arc::new(FlakeyProvider::new(5, "recovered"));
    let cb = CircuitBreakerProvider::new(
        flakey.clone(),
        CircuitBreakerConfig {
            failure_threshold: 3,
            recovery_timeout: Duration::from_millis(50),
            half_open_successes_needed: 1,
        },
    );

    // Send 3 failures to trip the breaker.
    for _ in 0..3 {
        let _ = cb.complete(make_request()).await;
    }

    // Circuit should now be open.
    let state = cb.circuit_state().await;
    assert_eq!(
        state,
        axinite::llm::circuit_breaker::CircuitState::Open,
        "circuit should be open after 3 failures"
    );

    // Requests while open should be rejected immediately with a circuit breaker message.
    let err = cb.complete(make_request()).await.unwrap_err();
    match &err {
        LlmError::RequestFailed { reason, .. } => {
            assert!(
                reason.contains("Circuit breaker open"),
                "expected circuit breaker message, got: {}",
                reason
            );
        }
        other => panic!("expected RequestFailed, got: {:?}", other),
    }

    // Wait for recovery timeout.
    tokio::time::sleep(Duration::from_millis(60)).await;

    // The FlakeyProvider still has 2 failures remaining (5 - 3 = 2).
    // The first probe (half-open) will fail, sending it back to open.
    let _ = cb.complete(make_request()).await;
    assert_eq!(
        cb.circuit_state().await,
        axinite::llm::circuit_breaker::CircuitState::Open,
        "probe failed, should reopen"
    );

    // Wait again for recovery.
    tokio::time::sleep(Duration::from_millis(60)).await;

    // Second probe: FlakeyProvider has 1 failure remaining.
    let _ = cb.complete(make_request()).await;
    assert_eq!(
        cb.circuit_state().await,
        axinite::llm::circuit_breaker::CircuitState::Open,
        "still one failure left, should reopen again"
    );

    // Wait once more.
    tokio::time::sleep(Duration::from_millis(60)).await;

    // Third probe: FlakeyProvider should now succeed (all 5 failures consumed).
    let result = cb.complete(make_request()).await;
    assert!(result.is_ok(), "should succeed after all failures consumed");
    assert_eq!(result.unwrap().content, "recovered");
    assert_eq!(
        cb.circuit_state().await,
        axinite::llm::circuit_breaker::CircuitState::Closed,
        "circuit should close after successful probe"
    );
}
