//! Cache key determinism and core caching behaviour tests
//! (hits, misses, TTL expiry, LRU eviction, and delegation).

use crate::llm::provider::{ChatMessage, CompletionRequest, ToolCompletionRequest};
use crate::llm::response_cache::*;
use crate::testing::StubLlm;

use super::{different_request, simple_request};

#[test]
fn cache_key_is_deterministic() {
    let req = simple_request();
    let k1 = cache_key("model-a", &req);
    let k2 = cache_key("model-a", &req);
    assert_eq!(k1, k2);
    assert_eq!(k1.len(), 64); // SHA-256 hex
}

#[test]
fn cache_key_varies_by_model() {
    let req = simple_request();
    let k1 = cache_key("model-a", &req);
    let k2 = cache_key("model-b", &req);
    assert_ne!(k1, k2);
}

#[test]
fn cache_key_varies_by_messages() {
    let k1 = cache_key("model-a", &simple_request());
    let k2 = cache_key("model-a", &different_request());
    assert_ne!(k1, k2);
}

#[test]
fn cache_key_varies_by_temperature() {
    let mut req_a = simple_request();
    req_a.temperature = Some(0.0);
    let mut req_b = simple_request();
    req_b.temperature = Some(1.0);
    assert_ne!(cache_key("m", &req_a), cache_key("m", &req_b));
}

#[test]
fn cache_key_varies_by_max_tokens() {
    let mut req_a = simple_request();
    req_a.max_tokens = Some(100);
    let mut req_b = simple_request();
    req_b.max_tokens = Some(500);
    assert_ne!(cache_key("m", &req_a), cache_key("m", &req_b));
}

#[tokio::test]
async fn cache_hit_avoids_provider_call() {
    let stub = Arc::new(StubLlm::new("cached response"));
    let cached = CachedProvider::new(
        stub.clone(),
        ResponseCacheConfig {
            ttl: Duration::from_secs(60),
            max_entries: 100,
        },
    );

    // First call: cache miss
    let r1 = cached.complete(simple_request()).await.unwrap();
    assert_eq!(stub.calls(), 1);
    assert_eq!(r1.content, "cached response");

    // Second call: cache hit
    let r2 = cached.complete(simple_request()).await.unwrap();
    assert_eq!(stub.calls(), 1); // still 1
    assert_eq!(r2.content, "cached response");

    assert_eq!(cached.total_hits(), 1);
}

#[tokio::test]
async fn different_messages_get_different_entries() {
    let stub = Arc::new(StubLlm::new("cached response"));
    let cached = CachedProvider::new(stub.clone(), ResponseCacheConfig::default());

    cached.complete(simple_request()).await.unwrap();
    cached.complete(different_request()).await.unwrap();

    assert_eq!(stub.calls(), 2);
    assert_eq!(cached.len(), 2);
}

#[tokio::test]
async fn expired_entries_are_evicted() {
    let stub = Arc::new(StubLlm::new("cached response"));
    let cached = CachedProvider::new(
        stub.clone(),
        ResponseCacheConfig {
            ttl: Duration::from_millis(1),
            max_entries: 100,
        },
    );

    cached.complete(simple_request()).await.unwrap();
    assert_eq!(stub.calls(), 1);

    // Wait for TTL to expire
    tokio::time::sleep(Duration::from_millis(10)).await;

    // Should be a cache miss now
    cached.complete(simple_request()).await.unwrap();
    assert_eq!(stub.calls(), 2);
}

#[tokio::test]
async fn lru_eviction_removes_oldest() {
    let stub = Arc::new(StubLlm::new("cached response"));
    let cached = CachedProvider::new(
        stub.clone(),
        ResponseCacheConfig {
            ttl: Duration::from_secs(60),
            max_entries: 2,
        },
    );

    // Fill cache with 2 entries
    cached.complete(simple_request()).await.unwrap();
    cached.complete(different_request()).await.unwrap();
    assert_eq!(cached.len(), 2);

    // Add a third: should evict the oldest
    let third = CompletionRequest {
        messages: vec![ChatMessage::user("third")],
        model: None,
        max_tokens: None,
        temperature: None,
        stop_sequences: None,
        metadata: Default::default(),
    };
    cached.complete(third).await.unwrap();
    assert_eq!(cached.len(), 2);
    assert_eq!(stub.calls(), 3);
}

#[tokio::test]
async fn tool_calls_are_never_cached() {
    let stub = Arc::new(StubLlm::new("cached response"));
    let cached = CachedProvider::new(stub.clone(), ResponseCacheConfig::default());

    let req = ToolCompletionRequest {
        messages: vec![ChatMessage::user("use tool")],
        tools: vec![],
        model: None,
        max_tokens: None,
        temperature: None,
        tool_choice: None,
        metadata: Default::default(),
    };

    cached.complete_with_tools(req.clone()).await.unwrap();
    cached.complete_with_tools(req).await.unwrap();

    // Both should have called through
    assert_eq!(stub.calls(), 2);
    assert!(cached.is_empty());
}

#[tokio::test]
async fn provider_errors_are_not_cached() {
    let stub = Arc::new(StubLlm::new("cached response"));
    let cached = CachedProvider::new(
        stub.clone(),
        ResponseCacheConfig {
            ttl: Duration::from_secs(60),
            max_entries: 100,
        },
    );

    stub.set_failing(true);
    let result = cached.complete(simple_request()).await;
    assert!(result.is_err());
    assert!(cached.is_empty());

    // After fixing the provider, should succeed and cache
    stub.set_failing(false);
    cached.complete(simple_request()).await.unwrap();
    assert_eq!(cached.len(), 1);
}

#[tokio::test]
async fn clear_empties_cache() {
    let stub = Arc::new(StubLlm::new("cached response"));
    let cached = CachedProvider::new(stub.clone(), ResponseCacheConfig::default());

    cached.complete(simple_request()).await.unwrap();
    assert_eq!(cached.len(), 1);

    cached.clear();
    assert!(cached.is_empty());
}

#[tokio::test]
async fn model_override_gets_distinct_cache_entries() {
    let stub = Arc::new(StubLlm::new("cached response"));
    let cached = CachedProvider::new(stub.clone(), ResponseCacheConfig::default());

    let mut req_a = simple_request();
    req_a.model = Some("model-a".to_string());
    let mut req_b = simple_request();
    req_b.model = Some("model-b".to_string());

    cached.complete(req_a).await.unwrap();
    cached.complete(req_b).await.unwrap();

    assert_eq!(stub.calls(), 2);
    assert_eq!(cached.len(), 2);
}

#[test]
fn default_config_is_reasonable() {
    let cfg = ResponseCacheConfig::default();
    assert_eq!(cfg.ttl, Duration::from_secs(3600));
    assert_eq!(cfg.max_entries, 1000);
}

#[tokio::test]
async fn delegates_model_name() {
    let stub = Arc::new(StubLlm::new("cached response"));
    let cached = CachedProvider::new(stub.clone(), ResponseCacheConfig::default());
    assert_eq!(cached.model_name(), "stub-model");
}
