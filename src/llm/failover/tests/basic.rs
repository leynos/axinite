//! Core failover sequencing, model tracking, and error classification tests.

use std::time::Duration;

use super::super::*;
use super::mocks::{MockProvider, MultiCallMockProvider, make_request, make_tool_request};

// Test 1: Primary succeeds, no failover occurs.
#[tokio::test]
async fn primary_succeeds_no_failover() {
    let primary = Arc::new(MockProvider::succeeding("primary", "primary response"));
    let fallback = Arc::new(MockProvider::succeeding("fallback", "fallback response"));

    let failover = FailoverProvider::new(vec![primary, fallback]).unwrap();

    let response = failover.complete(make_request()).await.unwrap();
    assert_eq!(response.content, "primary response");
}

// Test 2: Primary fails with retryable error, fallback succeeds.
#[tokio::test]
async fn primary_fails_retryable_fallback_succeeds() {
    let primary = Arc::new(MockProvider::failing_retryable("primary"));
    let fallback = Arc::new(MockProvider::succeeding("fallback", "fallback response"));

    let failover = FailoverProvider::new(vec![primary, fallback]).unwrap();

    let response = failover.complete(make_request()).await.unwrap();
    assert_eq!(response.content, "fallback response");
}

// Test 3: All providers fail, returns last error.
#[tokio::test]
async fn all_providers_fail_returns_last_error() {
    let primary = Arc::new(MockProvider::failing_retryable("primary"));
    let fallback = Arc::new(MockProvider::failing_retryable("fallback"));

    let failover = FailoverProvider::new(vec![primary, fallback]).unwrap();

    let err = failover.complete(make_request()).await.unwrap_err();
    match err {
        LlmError::RequestFailed { provider, .. } => {
            assert_eq!(provider, "fallback");
        }
        other => panic!("expected RequestFailed, got: {other:?}"),
    }
}

// Test 4: Non-retryable error fails immediately, no failover.
#[tokio::test]
async fn non_retryable_error_fails_immediately() {
    let primary = Arc::new(MockProvider::failing_non_retryable("primary"));
    let fallback = Arc::new(MockProvider::succeeding("fallback", "fallback response"));

    let failover = FailoverProvider::new(vec![primary, fallback]).unwrap();

    let err = failover.complete(make_request()).await.unwrap_err();
    match err {
        LlmError::AuthFailed { provider } => {
            assert_eq!(provider, "primary");
        }
        other => panic!("expected AuthFailed, got: {other:?}"),
    }
}

// Test 5: Three providers, first two fail (retryable), third succeeds.
#[tokio::test]
async fn three_providers_first_two_fail_third_succeeds() {
    let p1 = Arc::new(MockProvider::failing_retryable("provider-1"));
    let p2 = Arc::new(MockProvider::failing_rate_limited("provider-2"));
    let p3 = Arc::new(MockProvider::succeeding("provider-3", "third time lucky"));

    let failover = FailoverProvider::new(vec![p1, p2, p3]).unwrap();

    let response = failover.complete(make_request()).await.unwrap();
    assert_eq!(response.content, "third time lucky");
}

// Test: complete_with_tools follows same failover logic.
#[tokio::test]
async fn complete_with_tools_failover() {
    let primary = Arc::new(MockProvider::failing_retryable("primary"));
    let fallback = Arc::new(MockProvider::succeeding("fallback", "tools fallback"));

    let failover = FailoverProvider::new(vec![primary, fallback]).unwrap();

    let response = failover
        .complete_with_tools(make_tool_request())
        .await
        .unwrap();
    assert_eq!(response.content.as_deref(), Some("tools fallback"));
}

// Test: model_name and cost_per_token reflect the last-used provider.
#[tokio::test]
async fn model_name_and_cost_track_last_used_provider() {
    let fallback_cost = Decimal::new(15, 6); // 0.000015

    let primary = Arc::new(MockProvider::failing_retryable("primary-model"));
    let fallback = Arc::new(MockProvider::succeeding_with_cost(
        "fallback-model",
        "ok",
        fallback_cost,
        fallback_cost,
    ));

    let failover = FailoverProvider::new(vec![primary, fallback]).unwrap();

    // Before any call, defaults to primary (index 0).
    assert_eq!(failover.model_name(), "primary-model");
    assert_eq!(failover.cost_per_token(), (Decimal::ZERO, Decimal::ZERO));

    // After failover, should reflect the fallback provider.
    let _ = failover.complete(make_request()).await.unwrap();
    assert_eq!(failover.model_name(), "fallback-model");
    assert_eq!(failover.cost_per_token(), (fallback_cost, fallback_cost));
}

// Test: model reporting is request-scoped under concurrent requests.
#[tokio::test]
async fn effective_model_name_is_request_scoped_under_concurrency() {
    let config = CooldownConfig {
        cooldown_duration: Duration::from_secs(60),
        failure_threshold: 3,
    };
    let primary = Arc::new(MultiCallMockProvider::fail_then_ok("primary", 1));
    let fallback = Arc::new(MultiCallMockProvider::always_ok("fallback"));
    let failover =
        Arc::new(FailoverProvider::with_cooldown(vec![primary, fallback], config).unwrap());

    let (first_done_tx, first_done_rx) = tokio::sync::oneshot::channel::<()>();
    let (second_done_tx, second_done_rx) = tokio::sync::oneshot::channel::<()>();

    let failover_a = Arc::clone(&failover);
    let task_a = tokio::spawn(async move {
        // First request: primary fails once, fallback serves.
        let _ = failover_a.complete(make_request()).await.unwrap();
        let _ = first_done_tx.send(());

        // Wait until the second request finishes and updates global state.
        let _ = second_done_rx.await;
        failover_a.effective_model_name(None)
    });

    let failover_b = Arc::clone(&failover);
    let task_b = tokio::spawn(async move {
        let _ = first_done_rx.await;
        // Second request: primary now succeeds.
        let _ = failover_b.complete(make_request()).await.unwrap();
        let model = failover_b.effective_model_name(None);
        let _ = second_done_tx.send(());
        model
    });

    let model_b = task_b.await.unwrap();
    let model_a = task_a.await.unwrap();

    assert_eq!(model_a, "fallback");
    assert_eq!(model_b, "primary");
}

// Test: list_models aggregates from all providers.
#[tokio::test]
async fn list_models_aggregates_all() {
    let p1 = Arc::new(MockProvider::succeeding("model-a", "ok"));
    let p2 = Arc::new(MockProvider::succeeding("model-b", "ok"));

    let failover = FailoverProvider::new(vec![p1, p2]).unwrap();

    let models = failover.list_models().await.unwrap();
    assert!(models.contains(&"model-a".to_string()));
    assert!(models.contains(&"model-b".to_string()));
}

// Test: is_retryable correctly classifies errors.
#[test]
fn retryable_classification() {
    // Retryable
    assert!(is_retryable(&LlmError::RequestFailed {
        provider: "p".into(),
        reason: "err".into(),
    }));
    assert!(is_retryable(&LlmError::RateLimited {
        provider: "p".into(),
        retry_after: None,
    }));
    assert!(is_retryable(&LlmError::InvalidResponse {
        provider: "p".into(),
        reason: "bad json".into(),
    }));
    assert!(is_retryable(&LlmError::SessionRenewalFailed {
        provider: "p".into(),
        reason: "timeout".into(),
    }));
    assert!(is_retryable(&LlmError::Io(std::io::Error::new(
        std::io::ErrorKind::ConnectionReset,
        "reset"
    ))));

    // Non-retryable
    assert!(!is_retryable(&LlmError::AuthFailed {
        provider: "p".into(),
    }));
    assert!(!is_retryable(&LlmError::SessionExpired {
        provider: "p".into(),
    }));
    assert!(!is_retryable(&LlmError::ContextLengthExceeded {
        used: 100_000,
        limit: 50_000,
    }));
    assert!(!is_retryable(&LlmError::ModelNotAvailable {
        provider: "p".into(),
        model: "m".into(),
    }));
}

// Test: empty providers list returns error (not panic).
#[test]
fn empty_providers_returns_error() {
    let result = FailoverProvider::new(vec![]);
    assert!(result.is_err());
}

// Test: set_model propagates to all providers and active_model_name reflects change.
#[test]
fn set_model_propagates_to_all_providers() {
    let p1: Arc<MockProvider> = Arc::new(MockProvider::succeeding("model-a", "ok"));
    let p2: Arc<MockProvider> = Arc::new(MockProvider::succeeding("model-b", "ok"));

    let failover = FailoverProvider::new(vec![
        Arc::clone(&p1) as Arc<dyn LlmProvider>,
        Arc::clone(&p2) as Arc<dyn LlmProvider>,
    ])
    .unwrap();

    // Before: active_model_name delegates to last_used (index 0 = p1).
    assert_eq!(failover.active_model_name(), "model-a");

    // Switch model.
    failover.set_model("new-model").unwrap();

    // Both inner providers should reflect the change.
    assert_eq!(p1.active_model_name(), "new-model");
    assert_eq!(p2.active_model_name(), "new-model");

    // FailoverProvider itself should report the new model.
    assert_eq!(failover.active_model_name(), "new-model");
}
