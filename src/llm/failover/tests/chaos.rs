//! Provider chaos and failover edge-case tests (QA plans P2-4.1 and 2.6).

use std::time::Duration;

use super::super::*;
use super::mocks::{MultiCallMockProvider, make_request, make_tool_request};

#[tokio::test]
async fn hanging_provider_failover_to_healthy_one() {
    // When primary hangs, caller can timeout and the secondary should be reachable
    // on a fresh request. The failover itself doesn't timeout individual providers
    // (that's the HTTP client's job), but after the first provider enters cooldown
    // from repeated failures, the failover skips it.
    let p1 = Arc::new(MultiCallMockProvider::always_fail("p1-broken"));
    let p2 = Arc::new(MultiCallMockProvider::always_ok("p2-healthy"));

    let config = CooldownConfig {
        cooldown_duration: Duration::from_secs(60),
        failure_threshold: 1,
    };
    let failover = FailoverProvider::with_cooldown(vec![p1.clone(), p2.clone()], config).unwrap();

    // First request: p1 fails → cooldown, p2 succeeds.
    let r = failover.complete(make_request()).await.unwrap();
    assert_eq!(r.content, "p2-healthy ok");

    // Second request: p1 skipped (in cooldown), p2 serves directly.
    let prev_p1 = p1.call_count();
    let r = failover.complete(make_request()).await.unwrap();
    assert_eq!(r.content, "p2-healthy ok");
    assert_eq!(p1.call_count(), prev_p1, "p1 should be skipped in cooldown");
}

#[tokio::test]
async fn all_providers_fail_returns_error_not_panic() {
    let p1 = Arc::new(MultiCallMockProvider::always_fail("p1"));
    let p2 = Arc::new(MultiCallMockProvider::always_fail("p2"));
    let p3 = Arc::new(MultiCallMockProvider::always_fail("p3"));

    let failover = FailoverProvider::new(vec![p1 as Arc<dyn LlmProvider>, p2, p3]).unwrap();

    // Should return an error, not panic.
    let result = failover.complete(make_request()).await;
    assert!(result.is_err());
}

#[tokio::test]
async fn failover_with_tools_follows_same_path() {
    let p1 = Arc::new(MultiCallMockProvider::always_fail("p1"));
    let p2 = Arc::new(MultiCallMockProvider::always_ok("p2"));

    let failover = FailoverProvider::new(vec![p1 as Arc<dyn LlmProvider>, p2]).unwrap();

    let result = failover.complete_with_tools(make_tool_request()).await;
    assert!(result.is_ok());
    assert_eq!(result.unwrap().content.unwrap(), "p2 ok");
}

#[tokio::test]
async fn single_provider_failover_still_works() {
    let p1 = Arc::new(MultiCallMockProvider::always_ok("solo"));
    let failover = FailoverProvider::new(vec![p1 as Arc<dyn LlmProvider>]).unwrap();

    let result = failover.complete(make_request()).await;
    assert!(result.is_ok());
    assert_eq!(result.unwrap().content, "solo ok");
}

/// When all providers fail with retryable errors, the failover must
/// return a graceful error (not panic via .unwrap()/.expect()). Verify
/// the error content includes the last provider's identity.
#[tokio::test]
async fn test_failover_all_providers_fail_no_panic() {
    let p1 = Arc::new(MultiCallMockProvider::always_fail("alpha"));
    let p2 = Arc::new(MultiCallMockProvider::always_fail("beta"));
    let p3 = Arc::new(MultiCallMockProvider::always_fail("gamma"));

    let failover = FailoverProvider::new(vec![
        p1 as Arc<dyn LlmProvider>,
        p2 as Arc<dyn LlmProvider>,
        p3 as Arc<dyn LlmProvider>,
    ])
    .unwrap();

    // All three providers fail. Must return Err, not panic.
    let result = failover.complete(make_request()).await;
    assert!(result.is_err(), "should return error, not panic");
    let err = result.unwrap_err();
    match &err {
        LlmError::RequestFailed { provider, reason } => {
            // The last error should come from the last provider tried.
            assert_eq!(
                provider, "gamma",
                "error should identify the last provider tried"
            );
            assert!(
                reason.contains("failed"),
                "error reason should describe the failure: {}",
                reason
            );
        }
        other => panic!("expected RequestFailed, got: {:?}", other),
    }

    // Also test complete_with_tools follows the same graceful path.
    let p4 = Arc::new(MultiCallMockProvider::always_fail("delta"));
    let p5 = Arc::new(MultiCallMockProvider::always_fail("epsilon"));
    let failover2 =
        FailoverProvider::new(vec![p4 as Arc<dyn LlmProvider>, p5 as Arc<dyn LlmProvider>])
            .unwrap();

    let result = failover2.complete_with_tools(make_tool_request()).await;
    assert!(
        result.is_err(),
        "complete_with_tools should also return error, not panic"
    );
}

/// A single provider that always fails with no fallback available.
/// Verifies the failover returns the error from that provider and
/// does not panic or produce an "unreachable" invariant violation.
#[tokio::test]
async fn test_failover_with_single_provider_failing() {
    let solo = Arc::new(MultiCallMockProvider::always_fail("solo-broken"));
    let failover = FailoverProvider::new(vec![solo.clone() as Arc<dyn LlmProvider>]).unwrap();

    // First call: should return error from the solo provider.
    let result = failover.complete(make_request()).await;
    assert!(result.is_err());
    match result.unwrap_err() {
        LlmError::RequestFailed { provider, .. } => {
            assert_eq!(provider, "solo-broken");
        }
        other => panic!("expected RequestFailed, got: {:?}", other),
    }

    // After repeated failures, the single provider enters cooldown.
    // But since it's the only provider, the "never skip all" logic
    // should still try it (as the oldest-cooled provider).
    let config = CooldownConfig {
        cooldown_duration: Duration::from_secs(300),
        failure_threshold: 1,
    };
    let solo2 = Arc::new(MultiCallMockProvider::always_fail("solo-cd"));
    let failover2 =
        FailoverProvider::with_cooldown(vec![solo2.clone() as Arc<dyn LlmProvider>], config)
            .unwrap();

    // First call: fails, enters cooldown (threshold=1).
    let _ = failover2.complete(make_request()).await;
    assert_eq!(solo2.call_count(), 1);

    // Second call: provider is in cooldown, but it's the only one,
    // so "never skip all" should try it anyway.
    let result = failover2.complete(make_request()).await;
    assert!(result.is_err(), "should still fail but not panic");
    assert_eq!(
        solo2.call_count(),
        2,
        "sole provider should be retried despite cooldown"
    );

    // Third call: same behaviour, no state corruption.
    let result = failover2.complete(make_request()).await;
    assert!(result.is_err());
    assert_eq!(solo2.call_count(), 3);
}
