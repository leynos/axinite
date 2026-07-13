//! Unit tests for LLM provider failover behaviour.

use super::*;

use std::sync::atomic::AtomicU32;
use std::sync::{Mutex, RwLock};
use std::time::Duration;

use crate::llm::provider::{CompletionResponse, FinishReason, ToolCompletionResponse};

/// A mock LLM provider that returns a predetermined result.
struct MockProvider {
    name: String,
    active_model: RwLock<String>,
    input_cost: Decimal,
    output_cost: Decimal,
    complete_result: Mutex<Option<Result<CompletionResponse, LlmError>>>,
    tool_complete_result: Mutex<Option<Result<ToolCompletionResponse, LlmError>>>,
}

impl MockProvider {
    fn succeeding(name: &str, content: &str) -> Self {
        Self {
            name: name.to_string(),
            active_model: RwLock::new(name.to_string()),
            input_cost: Decimal::ZERO,
            output_cost: Decimal::ZERO,
            complete_result: Mutex::new(Some(Ok(CompletionResponse {
                content: content.to_string(),
                input_tokens: 10,
                output_tokens: 5,
                finish_reason: FinishReason::Stop,
                cache_read_input_tokens: 0,
                cache_creation_input_tokens: 0,
            }))),
            tool_complete_result: Mutex::new(Some(Ok(ToolCompletionResponse {
                content: Some(content.to_string()),
                tool_calls: vec![],
                input_tokens: 10,
                output_tokens: 5,
                finish_reason: FinishReason::Stop,
                cache_read_input_tokens: 0,
                cache_creation_input_tokens: 0,
            }))),
        }
    }

    fn succeeding_with_cost(
        name: &str,
        content: &str,
        input_cost: Decimal,
        output_cost: Decimal,
    ) -> Self {
        Self {
            input_cost,
            output_cost,
            ..Self::succeeding(name, content)
        }
    }

    fn failing_retryable(name: &str) -> Self {
        Self {
            name: name.to_string(),
            active_model: RwLock::new(name.to_string()),
            input_cost: Decimal::ZERO,
            output_cost: Decimal::ZERO,
            complete_result: Mutex::new(Some(Err(LlmError::RequestFailed {
                provider: name.to_string(),
                reason: "server error".to_string(),
            }))),
            tool_complete_result: Mutex::new(Some(Err(LlmError::RequestFailed {
                provider: name.to_string(),
                reason: "server error".to_string(),
            }))),
        }
    }

    fn failing_non_retryable(name: &str) -> Self {
        Self {
            name: name.to_string(),
            active_model: RwLock::new(name.to_string()),
            input_cost: Decimal::ZERO,
            output_cost: Decimal::ZERO,
            complete_result: Mutex::new(Some(Err(LlmError::AuthFailed {
                provider: name.to_string(),
            }))),
            tool_complete_result: Mutex::new(Some(Err(LlmError::AuthFailed {
                provider: name.to_string(),
            }))),
        }
    }

    fn failing_rate_limited(name: &str) -> Self {
        Self {
            name: name.to_string(),
            active_model: RwLock::new(name.to_string()),
            input_cost: Decimal::ZERO,
            output_cost: Decimal::ZERO,
            complete_result: Mutex::new(Some(Err(LlmError::RateLimited {
                provider: name.to_string(),
                retry_after: Some(Duration::from_secs(30)),
            }))),
            tool_complete_result: Mutex::new(Some(Err(LlmError::RateLimited {
                provider: name.to_string(),
                retry_after: Some(Duration::from_secs(30)),
            }))),
        }
    }
}

impl crate::llm::NativeLlmProvider for MockProvider {
    fn model_name(&self) -> &str {
        &self.name
    }

    fn cost_per_token(&self) -> (Decimal, Decimal) {
        (self.input_cost, self.output_cost)
    }

    async fn complete(&self, _request: CompletionRequest) -> Result<CompletionResponse, LlmError> {
        self.complete_result
            .lock()
            .unwrap()
            .take()
            .unwrap_or_else(|| {
                Err(LlmError::InvalidResponse {
                    provider: self.name.clone(),
                    reason: "MockProvider::complete called more than once".to_string(),
                })
            })
    }

    async fn complete_with_tools(
        &self,
        _request: ToolCompletionRequest,
    ) -> Result<ToolCompletionResponse, LlmError> {
        self.tool_complete_result
            .lock()
            .unwrap()
            .take()
            .unwrap_or_else(|| {
                Err(LlmError::InvalidResponse {
                    provider: self.name.clone(),
                    reason: "MockProvider::complete_with_tools called more than once".to_string(),
                })
            })
    }

    async fn list_models(&self) -> Result<Vec<String>, LlmError> {
        Ok(vec![self.name.clone()])
    }

    fn active_model_name(&self) -> String {
        self.active_model.read().unwrap().clone()
    }

    fn set_model(&self, model: &str) -> Result<(), LlmError> {
        *self.active_model.write().unwrap() = model.to_string();
        Ok(())
    }
}

fn make_request() -> CompletionRequest {
    CompletionRequest::new(vec![crate::llm::ChatMessage::user("hello")])
}

fn make_tool_request() -> ToolCompletionRequest {
    ToolCompletionRequest::new(vec![crate::llm::ChatMessage::user("hello")], vec![])
}

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

// --- MultiCallMockProvider for cooldown tests ---
//
// Unlike `MockProvider` which uses `.take()` (single-use), this mock
// tracks a call counter and returns errors for the first N calls,
// then succeeds.

struct MultiCallMockProvider {
    name: String,
    /// How many calls should fail before succeeding. 0 = always succeed.
    fail_count: u32,
    /// Atomically tracks how many times `complete` has been called.
    calls: AtomicU32,
    /// If true, failures are non-retryable (AuthFailed).
    non_retryable: bool,
}

impl MultiCallMockProvider {
    /// Always succeeds.
    fn always_ok(name: &str) -> Self {
        Self {
            name: name.to_string(),
            fail_count: 0,
            calls: AtomicU32::new(0),
            non_retryable: false,
        }
    }

    /// Fails with retryable error for the first `n` calls, then succeeds.
    fn fail_then_ok(name: &str, n: u32) -> Self {
        Self {
            name: name.to_string(),
            fail_count: n,
            calls: AtomicU32::new(0),
            non_retryable: false,
        }
    }

    /// Always fails with retryable error.
    fn always_fail(name: &str) -> Self {
        Self {
            name: name.to_string(),
            fail_count: u32::MAX,
            calls: AtomicU32::new(0),
            non_retryable: false,
        }
    }

    /// Always fails with non-retryable error.
    fn always_fail_non_retryable(name: &str) -> Self {
        Self {
            name: name.to_string(),
            fail_count: u32::MAX,
            calls: AtomicU32::new(0),
            non_retryable: true,
        }
    }

    fn call_count(&self) -> u32 {
        self.calls.load(Ordering::Relaxed)
    }
}

impl crate::llm::NativeLlmProvider for MultiCallMockProvider {
    fn model_name(&self) -> &str {
        &self.name
    }

    fn cost_per_token(&self) -> (Decimal, Decimal) {
        (Decimal::ZERO, Decimal::ZERO)
    }

    async fn complete(&self, _request: CompletionRequest) -> Result<CompletionResponse, LlmError> {
        let n = self.calls.fetch_add(1, Ordering::Relaxed);
        if n < self.fail_count {
            if self.non_retryable {
                return Err(LlmError::AuthFailed {
                    provider: self.name.clone(),
                });
            }
            return Err(LlmError::RequestFailed {
                provider: self.name.clone(),
                reason: format!("call {} failed", n),
            });
        }
        Ok(CompletionResponse {
            content: format!("{} ok", self.name),
            input_tokens: 10,
            output_tokens: 5,
            finish_reason: FinishReason::Stop,
            cache_read_input_tokens: 0,
            cache_creation_input_tokens: 0,
        })
    }

    async fn complete_with_tools(
        &self,
        _request: ToolCompletionRequest,
    ) -> Result<ToolCompletionResponse, LlmError> {
        let n = self.calls.fetch_add(1, Ordering::Relaxed);
        if n < self.fail_count {
            if self.non_retryable {
                return Err(LlmError::AuthFailed {
                    provider: self.name.clone(),
                });
            }
            return Err(LlmError::RequestFailed {
                provider: self.name.clone(),
                reason: format!("call {} failed", n),
            });
        }
        Ok(ToolCompletionResponse {
            content: Some(format!("{} ok", self.name)),
            tool_calls: vec![],
            input_tokens: 10,
            output_tokens: 5,
            finish_reason: FinishReason::Stop,
            cache_read_input_tokens: 0,
            cache_creation_input_tokens: 0,
        })
    }

    async fn list_models(&self) -> Result<Vec<String>, LlmError> {
        Ok(vec![self.name.clone()])
    }
}

// --- Cooldown tests ---

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

// Test: activate_cooldown(0) still activates cooldown (sentinel collision fix).
#[test]
fn cooldown_at_nanos_zero_still_activates() {
    let cd = ProviderCooldown::new();
    cd.activate_cooldown(0);
    assert!(cd.is_in_cooldown(0, 1000));
    assert_eq!(cd.cooldown_activated_nanos.load(Ordering::Relaxed), 1);
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

// === QA Plan P2 - 4.1: Provider chaos tests ===

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

// === QA Plan 2.6: Failover edge case tests ===

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

    // Third call: same behavior, no state corruption.
    let result = failover2.complete(make_request()).await;
    assert!(result.is_err());
    assert_eq!(solo2.call_count(), 3);
}
