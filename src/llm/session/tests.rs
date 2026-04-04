use super::*;
use crate::testing::credentials::{TEST_SESSION_NEARAI_ABC, TEST_SESSION_TOKEN};
use secrecy::ExposeSecret;
use tempfile::tempdir;

#[tokio::test]
async fn test_session_save_load() {
    let dir = tempdir().unwrap();
    let session_path = dir.path().join("session.json");

    let config = SessionConfig {
        auth_base_url: "https://example.com".to_string(),
        session_path: session_path.clone(),
    };

    let manager = SessionManager::new_async(config.clone()).await;

    assert!(!manager.has_token().await);

    manager
        .save_session(TEST_SESSION_TOKEN, Some("near"))
        .await
        .unwrap();
    manager
        .set_token(SecretString::from(TEST_SESSION_TOKEN))
        .await;

    assert!(manager.has_token().await);
    let token = manager.get_token().await.unwrap();
    assert_eq!(token.expose_secret(), TEST_SESSION_TOKEN);

    let manager2 = SessionManager::new_async(config).await;
    assert!(manager2.has_token().await);
    let token2 = manager2.get_token().await.unwrap();
    assert_eq!(token2.expose_secret(), TEST_SESSION_TOKEN);

    let data: SessionData =
        serde_json::from_str(&std::fs::read_to_string(&session_path).unwrap()).unwrap();
    assert_eq!(data.session_token, TEST_SESSION_TOKEN);
    assert_eq!(data.auth_provider, Some("near".to_string()));
}

#[tokio::test]
async fn test_get_token_without_auth_fails() {
    let dir = tempdir().unwrap();
    let config = SessionConfig {
        auth_base_url: "https://example.com".to_string(),
        session_path: dir.path().join("nonexistent.json"),
    };

    let manager = SessionManager::new_async(config).await;
    let result = manager.get_token().await;
    assert!(result.is_err());
    assert!(matches!(result, Err(LlmError::AuthFailed { .. })));
}

#[test]
fn test_session_data_serde_roundtrip_auth_provider_variants() {
    for auth_provider in [Some("github".to_string()), None] {
        let original = SessionData {
            session_token: TEST_SESSION_NEARAI_ABC.to_string(),
            created_at: Utc::now(),
            auth_provider: auth_provider.clone(),
        };
        let json = serde_json::to_string(&original).unwrap();
        let deserialized: SessionData = serde_json::from_str(&json).unwrap();

        assert_eq!(deserialized.session_token, original.session_token);
        assert_eq!(deserialized.auth_provider, auth_provider);
        assert_eq!(deserialized.created_at, original.created_at);
    }
}

#[test]
fn test_session_data_missing_auth_provider_defaults_to_none() {
    let json = r#"{"session_token":"tok_legacy","created_at":"2025-01-01T00:00:00Z"}"#;
    let data: SessionData = serde_json::from_str(json).unwrap();
    assert_eq!(data.session_token, "tok_legacy");
    assert_eq!(data.auth_provider, None);
}

#[test]
fn test_session_config_default() {
    let config = SessionConfig::default();
    assert_eq!(config.auth_base_url, "https://private.near.ai");
    assert!(config.session_path.ends_with("session.json"));
}

#[tokio::test]
async fn test_new_with_nonexistent_session_file() {
    let dir = tempdir().unwrap();
    let config = SessionConfig {
        auth_base_url: "https://example.com".to_string(),
        session_path: dir.path().join("does_not_exist.json"),
    };
    let manager = SessionManager::new(config);
    assert!(!manager.has_token().await);
}

#[tokio::test]
async fn test_set_token_get_token_roundtrip() {
    let dir = tempdir().unwrap();
    let config = SessionConfig {
        auth_base_url: "https://example.com".to_string(),
        session_path: dir.path().join("session.json"),
    };
    let manager = SessionManager::new(config);
    manager
        .set_token(SecretString::from("my_secret_token"))
        .await;
    let token = manager.get_token().await.unwrap();
    assert_eq!(token.expose_secret(), "my_secret_token");
}

#[tokio::test]
async fn test_has_token_false_then_true() {
    let dir = tempdir().unwrap();
    let config = SessionConfig {
        auth_base_url: "https://example.com".to_string(),
        session_path: dir.path().join("session.json"),
    };
    let manager = SessionManager::new(config);
    assert!(!manager.has_token().await);
    manager.set_token(SecretString::from("tok_something")).await;
    assert!(manager.has_token().await);
}

#[tokio::test]
async fn test_has_api_key_false_then_true() {
    let dir = tempdir().unwrap();
    let config = SessionConfig {
        auth_base_url: "https://example.com".to_string(),
        session_path: dir.path().join("session.json"),
    };
    let manager = SessionManager::new(config);

    assert!(!manager.has_api_key().await);
    manager.set_api_key(SecretString::from("sk_test")).await;
    assert!(manager.has_api_key().await);
}

#[tokio::test]
async fn test_get_api_key_returns_stored_secret() {
    let dir = tempdir().unwrap();
    let config = SessionConfig {
        auth_base_url: "https://example.com".to_string(),
        session_path: dir.path().join("session.json"),
    };
    let manager = SessionManager::new(config);

    manager.set_api_key(SecretString::from("sk_test")).await;

    let api_key = manager.get_api_key().await.expect("API key should exist");
    assert_eq!(api_key.expose_secret(), "sk_test");
}

#[tokio::test]
async fn test_ensure_authenticated_short_circuits_with_api_key() {
    let dir = tempdir().unwrap();
    let config = SessionConfig {
        auth_base_url: "https://example.com".to_string(),
        session_path: dir.path().join("session.json"),
    };
    let manager = SessionManager::new(config);

    manager.set_api_key(SecretString::from("sk_test")).await;

    manager
        .ensure_authenticated()
        .await
        .expect("API key auth should not require session login");
}

#[tokio::test]
async fn test_save_session_then_load_in_new_manager() {
    let dir = tempdir().unwrap();
    let session_path = dir.path().join("session.json");
    let config = SessionConfig {
        auth_base_url: "https://example.com".to_string(),
        session_path: session_path.clone(),
    };

    let manager = SessionManager::new_async(config.clone()).await;
    manager
        .save_session("persist_me", Some("google"))
        .await
        .unwrap();

    let manager2 = SessionManager::new_async(config).await;
    assert!(manager2.has_token().await);
    let token = manager2.get_token().await.unwrap();
    assert_eq!(token.expose_secret(), "persist_me");

    let raw: SessionData =
        serde_json::from_str(&std::fs::read_to_string(&session_path).unwrap()).unwrap();
    assert_eq!(raw.auth_provider, Some("google".to_string()));
}

#[tokio::test]
async fn test_save_session_with_no_auth_provider() {
    let dir = tempdir().unwrap();
    let session_path = dir.path().join("session.json");
    let config = SessionConfig {
        auth_base_url: "https://example.com".to_string(),
        session_path: session_path.clone(),
    };

    let manager = SessionManager::new_async(config).await;
    manager.save_session("anon_tok", None).await.unwrap();

    let raw: SessionData =
        serde_json::from_str(&std::fs::read_to_string(&session_path).unwrap()).unwrap();
    assert_eq!(raw.session_token, "anon_tok");
    assert_eq!(raw.auth_provider, None);
}

#[cfg(unix)]
#[tokio::test]
async fn test_session_file_permissions() {
    use std::os::unix::fs::PermissionsExt;

    let dir = tempdir().unwrap();
    let session_path = dir.path().join("session.json");
    let config = SessionConfig {
        auth_base_url: "https://example.com".to_string(),
        session_path: session_path.clone(),
    };

    let manager = SessionManager::new_async(config).await;
    manager
        .save_session("secret_tok", Some("github"))
        .await
        .unwrap();

    let metadata = std::fs::metadata(&session_path).unwrap();
    let mode = metadata.permissions().mode() & 0o777;
    assert_eq!(mode, 0o600, "Session file should have 0600 permissions");
}
