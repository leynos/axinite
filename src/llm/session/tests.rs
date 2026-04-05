//! Session-manager persistence and authentication-path tests.
//!
//! These tests cover disk-backed session storage plus API-key auth shortcuts.

use super::*;

#[cfg(feature = "libsql")]
use crate::db::NativeDatabase;
#[cfg(feature = "libsql")]
use crate::db::SettingsStore;
use crate::testing::credentials::{TEST_SESSION_NEARAI_ABC, TEST_SESSION_TOKEN};
use rstest::rstest;
use secrecy::ExposeSecret;
#[cfg(feature = "libsql")]
use std::sync::Arc;
use tempfile::{TempDir, tempdir};

enum SecretKind {
    Token,
    ApiKey,
}

fn mk_config(session_path: std::path::PathBuf) -> SessionConfig {
    SessionConfig {
        auth_base_url: "https://example.com".to_string(),
        session_path,
    }
}

fn new_mgr() -> (TempDir, SessionManager) {
    let dir = tempdir().expect("tempdir should be created");
    let manager = SessionManager::new(mk_config(dir.path().join("session.json")));
    (dir, manager)
}

async fn new_mgr_async() -> (TempDir, SessionManager) {
    let dir = tempdir().expect("tempdir should be created");
    let manager = SessionManager::new_async(mk_config(dir.path().join("session.json"))).await;
    (dir, manager)
}

#[tokio::test]
async fn test_session_save_load() {
    let dir = tempdir().expect("failed to create temp dir");
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
        .expect("failed to save session");
    manager
        .set_token(SecretString::from(TEST_SESSION_TOKEN))
        .await;

    assert!(manager.has_token().await);
    let token = manager
        .get_token()
        .await
        .expect("failed to get session token");
    assert_eq!(token.expose_secret(), TEST_SESSION_TOKEN);

    let manager2 = SessionManager::new_async(config).await;
    assert!(manager2.has_token().await);
    let token2 = manager2
        .get_token()
        .await
        .expect("failed to get reloaded session token");
    assert_eq!(token2.expose_secret(), TEST_SESSION_TOKEN);

    let content = tokio::fs::read_to_string(&session_path)
        .await
        .expect("failed to read saved session file");
    let data: SessionData =
        serde_json::from_str(&content).expect("failed to parse saved session data");
    assert_eq!(data.session_token, TEST_SESSION_TOKEN);
    assert_eq!(data.auth_provider, Some("near".to_string()));
}

#[tokio::test]
async fn test_get_token_without_auth_fails() {
    let dir = tempdir().expect("failed to create temp dir");
    let config = SessionConfig {
        auth_base_url: "https://example.com".to_string(),
        session_path: dir.path().join("nonexistent.json"),
    };

    let manager = SessionManager::new_async(config).await;
    let result = manager.get_token().await;
    assert!(result.is_err());
    assert!(matches!(result, Err(LlmError::AuthFailed { .. })));
}

#[rstest]
#[case(Some("github".to_string()))]
#[case(None)]
fn test_session_data_serde_roundtrip_auth_provider(#[case] auth_provider: Option<String>) {
    let original = SessionData {
        session_token: TEST_SESSION_NEARAI_ABC.to_string(),
        created_at: Utc::now(),
        auth_provider: auth_provider.clone(),
    };
    let json = serde_json::to_string(&original).expect("failed to serialize session data");
    let deserialized: SessionData =
        serde_json::from_str(&json).expect("failed to deserialize session data");

    assert_eq!(deserialized.session_token, original.session_token);
    assert_eq!(deserialized.auth_provider, auth_provider);
    assert_eq!(deserialized.created_at, original.created_at);
}

#[test]
fn test_session_data_missing_auth_provider_defaults_to_none() {
    let json = r#"{"session_token":"tok_legacy","created_at":"2025-01-01T00:00:00Z"}"#;
    let data: SessionData =
        serde_json::from_str(json).expect("failed to parse legacy session data");
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
    let dir = tempdir().expect("failed to create temp dir");
    let config = mk_config(dir.path().join("does_not_exist.json"));
    let manager = SessionManager::new(config);
    assert!(!manager.has_token().await);
}

#[rstest]
#[case(SecretKind::Token, "my_secret_token")]
#[case(SecretKind::ApiKey, "sk_test")]
#[tokio::test]
async fn test_secret_roundtrip(#[case] kind: SecretKind, #[case] secret: &str) {
    let (_dir, manager) = new_mgr_async().await;

    match kind {
        SecretKind::Token => {
            manager.set_token(SecretString::from(secret)).await;
            let token = manager.get_token().await.expect("token should exist");
            assert_eq!(token.expose_secret(), secret);
        }
        SecretKind::ApiKey => {
            manager.set_api_key(SecretString::from(secret)).await;
            let api_key = manager.get_api_key().await.expect("API key should exist");
            assert_eq!(api_key.expose_secret(), secret);
        }
    }
}

#[rstest]
#[case(SecretKind::Token, "tok_something")]
#[case(SecretKind::ApiKey, "sk_test")]
#[tokio::test]
async fn test_has_secret_false_then_true(#[case] kind: SecretKind, #[case] secret: &str) {
    let (_dir, manager) = new_mgr();

    match kind {
        SecretKind::Token => {
            assert!(!manager.has_token().await);
            manager.set_token(SecretString::from(secret)).await;
            assert!(manager.has_token().await);
        }
        SecretKind::ApiKey => {
            assert!(!manager.has_api_key().await);
            manager.set_api_key(SecretString::from(secret)).await;
            assert!(manager.has_api_key().await);
        }
    }
}

#[tokio::test]
async fn test_ensure_authenticated_short_circuits_with_api_key() {
    let (_dir, manager) = new_mgr_async().await;

    manager.set_api_key(SecretString::from("sk_test")).await;

    manager
        .ensure_authenticated()
        .await
        .expect("API key auth should not require session login");
}

#[cfg(feature = "libsql")]
#[tokio::test]
async fn test_save_session_persists_to_db_under_typed_setting_key() {
    let (dir, manager) = new_mgr_async().await;
    let db_path = dir.path().join("session.db");
    let backend = Arc::new(
        crate::db::libsql::LibSqlBackend::new_local(&db_path)
            .await
            .expect("libsql backend should be created"),
    );
    NativeDatabase::run_migrations(backend.as_ref())
        .await
        .expect("libsql migrations should succeed");
    manager.attach_store(backend.clone(), "session-user").await;

    manager
        .save_session("db-persisted-token", Some("github"))
        .await
        .expect("session should save to disk and DB");

    let saved = backend
        .get_setting(
            crate::db::UserId::from("session-user"),
            crate::db::SettingKey::from("nearai.session_token"),
        )
        .await
        .expect("typed DB session lookup should succeed")
        .expect("typed DB session should exist");
    assert_eq!(saved["session_token"], "db-persisted-token");
    assert_eq!(saved["auth_provider"], "github");
}

#[cfg(feature = "libsql")]
#[tokio::test]
async fn test_attach_store_loads_legacy_session_key_from_db() {
    let (dir, manager) = new_mgr_async().await;
    let db_path = dir.path().join("session.db");
    let backend = Arc::new(
        crate::db::libsql::LibSqlBackend::new_local(&db_path)
            .await
            .expect("libsql backend should be created"),
    );
    NativeDatabase::run_migrations(backend.as_ref())
        .await
        .expect("libsql migrations should succeed");
    backend
        .set_setting(
            crate::db::UserId::from("legacy-user"),
            crate::db::SettingKey::from("nearai.session"),
            &serde_json::json!({
                "session_token": "legacy-db-token",
                "created_at": "2025-01-01T00:00:00Z",
                "auth_provider": "google"
            }),
        )
        .await
        .expect("legacy DB session should seed");

    manager.attach_store(backend, "legacy-user").await;

    assert!(manager.has_token().await);
    let token = manager
        .get_token()
        .await
        .expect("legacy DB token should load through fallback");
    assert_eq!(token.expose_secret(), "legacy-db-token");
}

#[tokio::test]
async fn test_save_session_then_load_in_new_manager() {
    let dir = tempdir().expect("failed to create temp dir");
    let session_path = dir.path().join("session.json");
    let config = mk_config(session_path.clone());

    let manager = SessionManager::new_async(config.clone()).await;
    manager
        .save_session("persist_me", Some("google"))
        .await
        .expect("failed to save session");

    let manager2 = SessionManager::new_async(config).await;
    assert!(manager2.has_token().await);
    let token = manager2
        .get_token()
        .await
        .expect("failed to get persisted session token");
    assert_eq!(token.expose_secret(), "persist_me");

    let content = tokio::fs::read_to_string(&session_path)
        .await
        .expect("failed to read persisted session file");
    let raw: SessionData =
        serde_json::from_str(&content).expect("failed to parse persisted session data");
    assert_eq!(raw.auth_provider, Some("google".to_string()));
}

#[tokio::test]
async fn test_save_session_with_no_auth_provider() {
    let dir = tempdir().expect("failed to create temp dir");
    let session_path = dir.path().join("session.json");
    let config = mk_config(session_path.clone());

    let manager = SessionManager::new_async(config).await;
    manager
        .save_session("anon_tok", None)
        .await
        .expect("failed to save anonymous session");

    let content = tokio::fs::read_to_string(&session_path)
        .await
        .expect("failed to read anonymous session file");
    let raw: SessionData =
        serde_json::from_str(&content).expect("failed to parse anonymous session data");
    assert_eq!(raw.session_token, "anon_tok");
    assert_eq!(raw.auth_provider, None);
}

#[cfg(unix)]
#[tokio::test]
async fn test_session_file_permissions() {
    use std::os::unix::fs::PermissionsExt;

    let dir = tempdir().expect("failed to create temp dir");
    let session_path = dir.path().join("session.json");
    let config = mk_config(session_path.clone());

    let manager = SessionManager::new_async(config).await;
    manager
        .save_session("secret_tok", Some("github"))
        .await
        .expect("failed to save session with permissions");

    let metadata = std::fs::metadata(&session_path).expect("failed to stat session file");
    let mode = metadata.permissions().mode() & 0o777;
    assert_eq!(mode, 0o600, "Session file should have 0600 permissions");
}
