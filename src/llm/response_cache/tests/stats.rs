//! Tests for per-model cache slot isolation, hit accounting across
//! eviction, and periodic cache statistics logging.

use std::sync::atomic::Ordering;

use tracing_test::traced_test;

use crate::llm::provider::{ChatMessage, CompletionRequest};
use crate::llm::response_cache::*;
use crate::testing::StubLlm;

use super::{SwitchableStub, different_request, simple_request};

/// Switching models preserves existing cached entries and routes subsequent
/// requests to a separate cache slot. Switching back replays the old slot.
#[tokio::test]
async fn set_model_isolates_per_model_via_key() {
    let stub = Arc::new(SwitchableStub::new());
    let cached = CachedProvider::new(stub.clone(), ResponseCacheConfig::default());

    // Populate cache under the initial model ("stub-model").
    cached.complete(simple_request()).await.unwrap();
    assert_eq!(stub.call_count.load(Ordering::Relaxed), 1);
    assert_eq!(cached.len(), 1, "one entry cached for stub-model");

    // Switch to a different model — old entries must survive.
    cached.set_model("model-b").unwrap();
    assert_eq!(cached.len(), 1, "old entries preserved after model switch");

    // Same request under model-b is a cache miss (different key).
    cached.complete(simple_request()).await.unwrap();
    assert_eq!(
        stub.call_count.load(Ordering::Relaxed),
        2,
        "cache miss for model-b"
    );
    assert_eq!(cached.len(), 2, "separate slots for stub-model and model-b");

    // Switch back — original slot is still valid (cache hit, no extra call).
    cached.set_model("stub-model").unwrap();
    cached.complete(simple_request()).await.unwrap();
    assert_eq!(
        stub.call_count.load(Ordering::Relaxed),
        2,
        "cache hit when switching back to stub-model"
    );
}

/// When `set_model()` fails the error is propagated and the cache is unaffected.
#[tokio::test]
async fn set_model_error_leaves_cache_intact() {
    // StubLlm does not override set_model() — returns an error by default.
    let stub = Arc::new(StubLlm::default());
    let cached = CachedProvider::new(stub, ResponseCacheConfig::default());

    cached.complete(simple_request()).await.unwrap();
    assert_eq!(cached.len(), 1);

    let result = cached.set_model("new-model");
    assert!(result.is_err());
    assert_eq!(cached.len(), 1, "cache unaffected by failed set_model");
}

/// `hit_rate_pct` stays accurate even after entries are evicted.
/// The `total_hit_count` atomic is never decremented on eviction.
#[tokio::test]
async fn total_hits_survives_eviction() {
    let stub = Arc::new(StubLlm::new("response"));
    // max_entries = 1 so the first entry is LRU-evicted when a second arrives.
    let cached = CachedProvider::new(
        stub.clone(),
        ResponseCacheConfig {
            ttl: Duration::from_secs(60),
            max_entries: 1,
        },
    );

    // Populate the cache and score a hit.
    cached.complete(simple_request()).await.unwrap();
    cached.complete(simple_request()).await.unwrap();
    assert_eq!(cached.total_hits(), 1);

    // Add a different request — LRU evicts the first entry.
    cached.complete(different_request()).await.unwrap();
    assert_eq!(cached.len(), 1, "first entry was evicted");

    // The hit from the evicted entry must still be counted.
    assert_eq!(cached.total_hits(), 1, "hit count survives eviction");
}

/// A stats line is emitted exactly at the 100th request.
#[tokio::test]
#[traced_test]
async fn stats_logged_at_request_100() {
    let stub = Arc::new(StubLlm::new("response"));
    let cached = CachedProvider::new(
        stub.clone(),
        ResponseCacheConfig {
            ttl: Duration::from_secs(60),
            max_entries: 2000,
        },
    );

    // 99 distinct requests — no stats line yet.
    for i in 0..99u32 {
        let req = CompletionRequest {
            messages: vec![ChatMessage::user(format!("request {i}"))],
            model: None,
            max_tokens: None,
            temperature: None,
            stop_sequences: None,
            metadata: Default::default(),
        };
        cached.complete(req).await.unwrap();
    }
    assert!(
        !logs_contain("LLM response cache statistics"),
        "no stats before request 100"
    );

    // 100th request triggers the first stats line.
    let req = CompletionRequest {
        messages: vec![ChatMessage::user("request 99")],
        model: None,
        max_tokens: None,
        temperature: None,
        stop_sequences: None,
        metadata: Default::default(),
    };
    cached.complete(req).await.unwrap();
    assert!(
        logs_contain("LLM response cache statistics"),
        "stats emitted at request 100"
    );
}

/// Stats are emitted even when the inner provider returns an error.
#[tokio::test]
#[traced_test]
async fn stats_logged_on_provider_error_at_interval() {
    let stub = Arc::new(StubLlm::new("response"));
    let cached = CachedProvider::new(
        stub.clone(),
        ResponseCacheConfig {
            ttl: Duration::from_secs(60),
            max_entries: 2000,
        },
    );

    // 99 successful requests.
    for i in 0..99u32 {
        let req = CompletionRequest {
            messages: vec![ChatMessage::user(format!("req {i}"))],
            model: None,
            max_tokens: None,
            temperature: None,
            stop_sequences: None,
            metadata: Default::default(),
        };
        cached.complete(req).await.unwrap();
    }

    // 100th request fails — stats must still be logged.
    stub.set_failing(true);
    let req = CompletionRequest {
        messages: vec![ChatMessage::user("req 99")],
        model: None,
        max_tokens: None,
        temperature: None,
        stop_sequences: None,
        metadata: Default::default(),
    };
    let result = cached.complete(req).await;
    assert!(result.is_err());
    assert!(
        logs_contain("LLM response cache statistics"),
        "stats emitted even when provider errors on request 100"
    );
}
