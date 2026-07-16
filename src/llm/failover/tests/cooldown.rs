//! Cooldown activation, expiry, and threshold behaviour tests.

use std::time::Duration;

use super::super::*;
use super::mocks::{MultiCallMockProvider, make_request};

// Cooldown test 1: Provider enters cooldown after `threshold` consecutive failures.
#[tokio::test]
async fn cooldown_activates_after_threshold() {
    let config = CooldownConfig {
        cooldown_duration: Duration::from_secs(300),
        failure_threshold: 2,
    };
    let p1 = Arc::new(MultiCallMockProvider::always_fail("p1"));
    let p2 = Arc::new(MultiCallMockProvider::always_ok("p2"));

    let failover = FailoverProvider::with_cooldown(vec![p1.clone(), p2.clone()], config).unwrap();

    // Request 1: p1 fails (count=1, below threshold), p2 succeeds.
    let r = failover.complete(make_request()).await.unwrap();
    assert_eq!(r.content, "p2 ok");
    assert_eq!(p1.call_count(), 1);

    // Request 2: p1 fails again (count=2, reaches threshold → cooldown), p2 succeeds.
    let r = failover.complete(make_request()).await.unwrap();
    assert_eq!(r.content, "p2 ok");
    assert_eq!(p1.call_count(), 2);

    // Request 3: p1 should be skipped (in cooldown), only p2 called.
    let prev_p1_calls = p1.call_count();
    let r = failover.complete(make_request()).await.unwrap();
    assert_eq!(r.content, "p2 ok");
    // p1 was NOT called again.
    assert_eq!(p1.call_count(), prev_p1_calls);
}

// Cooldown test 2: Cooldown expires after duration, provider is retried.
#[tokio::test]
async fn cooldown_expires_after_duration() {
    let config = CooldownConfig {
        cooldown_duration: Duration::from_millis(1),
        failure_threshold: 1,
    };
    // p1 fails once then succeeds (fail_then_ok with n=1 would work,
    // but we use always_fail to prove it's skipped, then swap).
    let p1 = Arc::new(MultiCallMockProvider::fail_then_ok("p1", 2));
    let p2 = Arc::new(MultiCallMockProvider::always_ok("p2"));

    let failover = FailoverProvider::with_cooldown(vec![p1.clone(), p2.clone()], config).unwrap();

    // Request 1: p1 fails (threshold=1, enters cooldown immediately), p2 succeeds.
    let r = failover.complete(make_request()).await.unwrap();
    assert_eq!(r.content, "p2 ok");
    assert_eq!(p1.call_count(), 1);

    // Request 2: p1 in cooldown, skipped. Only p2 called.
    // (But cooldown is 1ms, so wait a bit to let it expire.)
    tokio::time::sleep(Duration::from_millis(5)).await;

    // After sleep, cooldown should have expired. p1 gets tried again.
    // p1 is set to fail 2 times total, so call #2 (index 1) still fails.
    // But it proves p1 was attempted again after cooldown expired.
    let r = failover.complete(make_request()).await.unwrap();
    assert_eq!(p1.call_count(), 2); // p1 was retried
    assert_eq!(r.content, "p2 ok"); // p2 handled it

    // Wait again for cooldown to expire, p1 call #3 (index 2) succeeds.
    tokio::time::sleep(Duration::from_millis(5)).await;
    let r = failover.complete(make_request()).await.unwrap();
    assert_eq!(r.content, "p1 ok");
    assert_eq!(p1.call_count(), 3);
}

// Cooldown test 3: Never skip all providers — oldest-cooled one is tried.
#[tokio::test]
async fn never_skip_all_providers() {
    let config = CooldownConfig {
        cooldown_duration: Duration::from_secs(300),
        failure_threshold: 1,
    };
    // Both providers always fail.
    let p1 = Arc::new(MultiCallMockProvider::always_fail("p1"));
    let p2 = Arc::new(MultiCallMockProvider::always_fail("p2"));

    let failover = FailoverProvider::with_cooldown(vec![p1.clone(), p2.clone()], config).unwrap();

    // Request 1: both tried, both fail, both enter cooldown.
    let _ = failover.complete(make_request()).await;
    assert_eq!(p1.call_count(), 1);
    assert_eq!(p2.call_count(), 1);

    // Request 2: all in cooldown, but the oldest-cooled one (p1, activated
    // first) should be tried.
    let prev_total = p1.call_count() + p2.call_count();
    let _ = failover.complete(make_request()).await;
    let new_total = p1.call_count() + p2.call_count();
    // Exactly one more call was made (to the oldest-cooled provider).
    assert_eq!(new_total, prev_total + 1);
}

// Cooldown test 4: Success resets failure count so it never reaches threshold.
//
// With threshold=3, accumulate 2 failures then succeed. Verify the
// atomic counter is back to 0 and no cooldown was activated. Then
// use a second provider pair to show that without the reset, 3
// consecutive failures DO trigger cooldown (control case).
#[tokio::test]
async fn reset_on_success() {
    let config = CooldownConfig {
        cooldown_duration: Duration::from_secs(300),
        failure_threshold: 3,
    };
    // p1 fails for calls 0,1 then succeeds on call 2+.
    let p1 = Arc::new(MultiCallMockProvider::fail_then_ok("p1", 2));
    let p2 = Arc::new(MultiCallMockProvider::always_ok("p2"));

    let failover =
        FailoverProvider::with_cooldown(vec![p1.clone(), p2.clone()], config.clone()).unwrap();

    // Request 1: p1 fails (failure_count=1), p2 succeeds.
    let r = failover.complete(make_request()).await.unwrap();
    assert_eq!(r.content, "p2 ok");

    // Request 2: p1 fails (failure_count=2, still below threshold=3), p2 succeeds.
    let r = failover.complete(make_request()).await.unwrap();
    assert_eq!(r.content, "p2 ok");
    assert_eq!(p1.call_count(), 2);

    // Request 3: p1 succeeds (call index 2) → counter resets to 0.
    let r = failover.complete(make_request()).await.unwrap();
    assert_eq!(r.content, "p1 ok");
    assert_eq!(p1.call_count(), 3);

    // Verify counter was reset to 0 and no cooldown activated.
    let nanos = failover.now_nanos();
    let cooldown_nanos = failover.cooldown_config.cooldown_duration.as_nanos() as u64;
    assert!(!failover.cooldowns[0].is_in_cooldown(nanos, cooldown_nanos));
    assert_eq!(
        failover.cooldowns[0].failure_count.load(Ordering::Relaxed),
        0
    );

    // Control: without a success in the middle, 3 failures DO trigger cooldown.
    let p3 = Arc::new(MultiCallMockProvider::always_fail("p3"));
    let p4 = Arc::new(MultiCallMockProvider::always_ok("p4"));
    let control = FailoverProvider::with_cooldown(vec![p3.clone(), p4.clone()], config).unwrap();
    for _ in 0..3 {
        let _ = control.complete(make_request()).await.unwrap();
    }
    let nanos = control.now_nanos();
    assert!(control.cooldowns[0].is_in_cooldown(nanos, cooldown_nanos));
}

// Cooldown test 5: threshold-1 failures don't trigger cooldown, threshold does.
#[tokio::test]
async fn threshold_boundary() {
    let config = CooldownConfig {
        cooldown_duration: Duration::from_secs(300),
        failure_threshold: 3,
    };
    let p1 = Arc::new(MultiCallMockProvider::always_fail("p1"));
    let p2 = Arc::new(MultiCallMockProvider::always_ok("p2"));

    let failover = FailoverProvider::with_cooldown(vec![p1.clone(), p2.clone()], config).unwrap();

    // 2 requests: p1 fails twice (below threshold of 3), not in cooldown.
    for _ in 0..2 {
        let r = failover.complete(make_request()).await.unwrap();
        assert_eq!(r.content, "p2 ok");
    }
    assert_eq!(p1.call_count(), 2);

    // p1 should still be available (not in cooldown).
    let nanos = failover.now_nanos();
    let cooldown_nanos = failover.cooldown_config.cooldown_duration.as_nanos() as u64;
    assert!(!failover.cooldowns[0].is_in_cooldown(nanos, cooldown_nanos));

    // 3rd request: p1 fails → reaches threshold → enters cooldown.
    let r = failover.complete(make_request()).await.unwrap();
    assert_eq!(r.content, "p2 ok");
    assert_eq!(p1.call_count(), 3);

    let nanos = failover.now_nanos();
    assert!(failover.cooldowns[0].is_in_cooldown(nanos, cooldown_nanos));

    // 4th request: p1 should be skipped.
    let prev = p1.call_count();
    let r = failover.complete(make_request()).await.unwrap();
    assert_eq!(r.content, "p2 ok");
    assert_eq!(p1.call_count(), prev); // not called
}

// Cooldown test 6: Non-retryable error returns immediately, no failure bump.
#[tokio::test]
async fn non_retryable_does_not_increment_cooldown() {
    let config = CooldownConfig {
        cooldown_duration: Duration::from_secs(300),
        failure_threshold: 1,
    };
    let p1 = Arc::new(MultiCallMockProvider::always_fail_non_retryable("p1"));
    let p2 = Arc::new(MultiCallMockProvider::always_ok("p2"));

    let failover = FailoverProvider::with_cooldown(vec![p1.clone(), p2.clone()], config).unwrap();

    // Non-retryable error should return immediately.
    let err = failover.complete(make_request()).await.unwrap_err();
    assert!(matches!(err, LlmError::AuthFailed { .. }));
    assert_eq!(p1.call_count(), 1);
    // p2 should NOT have been called (non-retryable = no failover).
    assert_eq!(p2.call_count(), 0);

    // p1 should NOT be in cooldown (non-retryable doesn't bump count).
    let nanos = failover.now_nanos();
    let cooldown_nanos = failover.cooldown_config.cooldown_duration.as_nanos() as u64;
    assert!(!failover.cooldowns[0].is_in_cooldown(nanos, cooldown_nanos));
}

// Cooldown test 7: Three providers, first in cooldown, second/third available.
#[tokio::test]
async fn three_providers_mixed_cooldown() {
    let config = CooldownConfig {
        cooldown_duration: Duration::from_secs(300),
        failure_threshold: 1,
    };
    let p1 = Arc::new(MultiCallMockProvider::always_fail("p1"));
    let p2 = Arc::new(MultiCallMockProvider::always_ok("p2"));
    let p3 = Arc::new(MultiCallMockProvider::always_ok("p3"));

    let failover =
        FailoverProvider::with_cooldown(vec![p1.clone(), p2.clone(), p3.clone()], config).unwrap();

    // Request 1: p1 fails → enters cooldown (threshold=1), p2 succeeds.
    let r = failover.complete(make_request()).await.unwrap();
    assert_eq!(r.content, "p2 ok");
    assert_eq!(p1.call_count(), 1);

    // Request 2: p1 skipped (cooldown), p2 and p3 available.
    let prev = p1.call_count();
    let r = failover.complete(make_request()).await.unwrap();
    assert_eq!(r.content, "p2 ok");
    assert_eq!(p1.call_count(), prev); // p1 skipped
}

// Test: activate_cooldown(0) still activates cooldown (sentinel collision fix).
#[test]
fn cooldown_at_nanos_zero_still_activates() {
    let cd = ProviderCooldown::new();
    cd.activate_cooldown(0);
    assert!(cd.is_in_cooldown(0, 1000));
    assert_eq!(cd.cooldown_activated_nanos.load(Ordering::Relaxed), 1);
}
