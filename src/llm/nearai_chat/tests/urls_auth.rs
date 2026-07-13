//! Tests for API URL construction and Bearer-token resolution priority.

use super::super::*;
use super::{test_nearai_config, test_session};

#[test]
fn test_api_url_with_base_without_v1() {
    let mut cfg = test_nearai_config("http://127.0.0.1:8318");

    let provider = NearAiChatProvider::new(cfg.clone(), test_session()).expect("provider");
    assert_eq!(
        provider.api_url("chat/completions"),
        "http://127.0.0.1:8318/v1/chat/completions"
    );

    cfg.base_url = "http://127.0.0.1:8318/".to_string();
    let provider = NearAiChatProvider::new(cfg, test_session()).expect("provider");
    assert_eq!(
        provider.api_url("/chat/completions"),
        "http://127.0.0.1:8318/v1/chat/completions"
    );
}

#[test]
fn test_api_url_with_base_already_v1() {
    let cfg = test_nearai_config("http://127.0.0.1:8318/v1");

    let provider = NearAiChatProvider::new(cfg, test_session()).expect("provider");
    assert_eq!(
        provider.api_url("chat/completions"),
        "http://127.0.0.1:8318/v1/chat/completions"
    );
}

#[tokio::test]
async fn test_resolve_bearer_token_config_api_key() {
    // When config.api_key is set, it takes top priority.
    let cfg = test_nearai_config("http://localhost:8318");
    let provider = NearAiChatProvider::new(cfg, test_session()).expect("provider");
    let token = provider
        .resolve_bearer_token()
        .await
        .expect("should resolve");
    assert_eq!(token, "test-key");
}

#[tokio::test]
async fn test_resolve_bearer_token_session_token() {
    // When config.api_key is None but session has a token, use session token.
    let mut cfg = test_nearai_config("http://localhost:8318");
    cfg.api_key = None;
    let session = test_session();
    session
        .set_token(secrecy::SecretString::from("session-tok-123".to_string()))
        .await;
    let provider = NearAiChatProvider::new(cfg, session).expect("provider");
    let token = provider
        .resolve_bearer_token()
        .await
        .expect("should resolve");
    assert_eq!(token, "session-tok-123");
}

#[tokio::test]
async fn test_resolve_bearer_token_session_beats_env_var() {
    // Session token takes priority over a session-managed API key.
    // This prevents unexpected auth mode switches mid-run.
    let mut cfg = test_nearai_config("http://localhost:8318");
    cfg.api_key = None;
    let session = test_session();
    session
        .set_token(secrecy::SecretString::from("oauth-token".to_string()))
        .await;
    session
        .set_api_key(secrecy::SecretString::from(
            "session-api-key-should-not-win".to_string(),
        ))
        .await;

    let provider = NearAiChatProvider::new(cfg, session).expect("provider");
    let token = provider
        .resolve_bearer_token()
        .await
        .expect("should resolve");
    assert_eq!(
        token, "oauth-token",
        "session token must take priority over session API key"
    );
}

#[tokio::test]
async fn test_resolve_bearer_token_config_beats_session_and_env() {
    // Config API key should win even when session token and session API key
    // are both present.
    let cfg = test_nearai_config("http://localhost:8318");
    let session = test_session();
    session
        .set_token(secrecy::SecretString::from("session-tok".to_string()))
        .await;
    session
        .set_api_key(secrecy::SecretString::from("session-api-key".to_string()))
        .await;

    let provider = NearAiChatProvider::new(cfg, session).expect("provider");
    let token = provider
        .resolve_bearer_token()
        .await
        .expect("should resolve");
    assert_eq!(
        token, "test-key",
        "config api_key must win over session token and session API key"
    );
}

// -- api_url edge cases ---------------------------------------------------

#[test]
fn test_api_url_with_trailing_v1_slash() {
    let cfg = test_nearai_config("http://example.com/v1/");
    let provider = NearAiChatProvider::new(cfg, test_session()).expect("provider");
    // Trailing slash gets trimmed, then /v1 is detected
    assert_eq!(provider.api_url("models"), "http://example.com/v1/models");
}

#[test]
fn test_api_url_with_deep_base_path() {
    let cfg = test_nearai_config("http://example.com/api/proxy");
    let provider = NearAiChatProvider::new(cfg, test_session()).expect("provider");
    assert_eq!(
        provider.api_url("chat/completions"),
        "http://example.com/api/proxy/v1/chat/completions"
    );
}
