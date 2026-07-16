//! Tests for provider model listing helpers and their fallbacks.

use super::super::model_catalogue::{
    AnthropicAuth, fetch_anthropic_models, fetch_ollama_models, fetch_openai_models,
    is_openai_chat_model, resolve_anthropic_auth, sort_openai_models,
};
use super::helpers::{EnvGuard, OverlayGuard};

#[tokio::test]
async fn test_fetch_anthropic_models_static_fallback() {
    // With no API key, should return static defaults
    let _guard = EnvGuard::clear("ANTHROPIC_API_KEY");
    let models = fetch_anthropic_models(None).await;
    assert!(!models.is_empty());
    assert!(
        models.iter().any(|(id, _)| id.contains("claude")),
        "static defaults should include a Claude model"
    );
}

#[test]
fn test_resolve_anthropic_auth_treats_cached_oauth_as_oauth() {
    let auth = resolve_anthropic_auth(Some("sk-ant-oat01-test-token"));
    assert!(matches!(auth, Some(AnthropicAuth::OAuth(_))));
}

#[test]
fn test_resolve_anthropic_auth_reads_api_key_from_overlay_helper() {
    let _guard = OverlayGuard::set("ANTHROPIC_API_KEY", "sk-ant-api-test");
    let auth = resolve_anthropic_auth(None);
    assert!(matches!(auth, Some(AnthropicAuth::ApiKey(_))));
}

#[tokio::test]
async fn test_fetch_openai_models_static_fallback() {
    let _guard = EnvGuard::clear("OPENAI_API_KEY");
    let models = fetch_openai_models(None).await;
    assert!(!models.is_empty());
    assert_eq!(models[0].0, "gpt-5.3-codex");
    assert!(
        models.iter().any(|(id, _)| id.contains("gpt")),
        "static defaults should include a GPT model"
    );
}

#[test]
fn test_is_openai_chat_model_includes_gpt5_and_filters_non_chat_variants() {
    assert!(is_openai_chat_model("gpt-5"));
    assert!(is_openai_chat_model("gpt-5-mini-2026-01-01"));
    assert!(is_openai_chat_model("o3-2025-04-16"));
    assert!(!is_openai_chat_model("chatgpt-image-latest"));
    assert!(!is_openai_chat_model("gpt-4o-realtime-preview"));
    assert!(!is_openai_chat_model("gpt-4o-mini-transcribe"));
    assert!(!is_openai_chat_model("text-embedding-3-large"));
}

#[test]
fn test_sort_openai_models_prioritizes_best_models_first() {
    let mut models = vec![
        ("gpt-4o-mini".to_string(), "gpt-4o-mini".to_string()),
        ("gpt-5-mini".to_string(), "gpt-5-mini".to_string()),
        ("o3".to_string(), "o3".to_string()),
        ("gpt-4.1".to_string(), "gpt-4.1".to_string()),
        ("gpt-5".to_string(), "gpt-5".to_string()),
    ];

    sort_openai_models(&mut models);

    let ordered: Vec<String> = models.into_iter().map(|(id, _)| id).collect();
    assert_eq!(
        ordered,
        vec![
            "gpt-5".to_string(),
            "gpt-5-mini".to_string(),
            "o3".to_string(),
            "gpt-4.1".to_string(),
            "gpt-4o-mini".to_string(),
        ]
    );
}

#[tokio::test]
async fn test_fetch_ollama_models_unreachable_fallback() {
    // Point at a port nothing listens on
    let models = fetch_ollama_models("http://127.0.0.1:1").await;
    assert!(!models.is_empty(), "should fall back to static defaults");
}
