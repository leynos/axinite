//! Tests for host-credential resolution and OAuth refresh skipping.

use super::super::*;
use crate::testing::credentials::{
    TEST_GOOGLE_OAUTH_FRESH, TEST_GOOGLE_OAUTH_LEGACY, TEST_GOOGLE_OAUTH_TOKEN,
    TEST_OAUTH_CLIENT_ID, TEST_OAUTH_CLIENT_SECRET, test_secrets_store,
};

#[tokio::test]
async fn test_resolve_host_credentials_no_store() {
    use crate::tools::wasm::wrapper::resolve_host_credentials;

    let caps = Capabilities::default();
    let result = resolve_host_credentials(&caps, None, "user1", None).await;
    assert!(result.is_empty());
}

#[tokio::test]
async fn test_resolve_host_credentials_no_http_cap() {
    use crate::tools::wasm::wrapper::resolve_host_credentials;

    let store = test_secrets_store();

    let caps = Capabilities::default();
    let result = resolve_host_credentials(&caps, Some(&store), "user1", None).await;
    assert!(result.is_empty());
}

#[tokio::test]
async fn test_resolve_host_credentials_bearer() {
    use std::collections::HashMap;

    use crate::secrets::{CreateSecretParams, CredentialLocation, CredentialMapping, SecretsStore};
    use crate::tools::wasm::capabilities::HttpCapability;
    use crate::tools::wasm::wrapper::resolve_host_credentials;

    let store = test_secrets_store();

    store
        .create(
            "user1",
            CreateSecretParams::new("google_oauth_token", TEST_GOOGLE_OAUTH_TOKEN),
        )
        .await
        .unwrap();

    let mut credentials = HashMap::new();
    credentials.insert(
        "google_oauth_token".to_string(),
        CredentialMapping {
            secret_name: "google_oauth_token".to_string(),
            location: CredentialLocation::AuthorizationBearer,
            host_patterns: vec!["www.googleapis.com".to_string()],
        },
    );

    let caps = Capabilities {
        http: Some(HttpCapability {
            credentials,
            ..Default::default()
        }),
        ..Default::default()
    };

    let result = resolve_host_credentials(&caps, Some(&store), "user1", None).await;
    assert_eq!(result.len(), 1);
    assert_eq!(result[0].host_patterns, vec!["www.googleapis.com"]);
    assert_eq!(
        result[0].headers.get("Authorization"),
        Some(&format!("Bearer {TEST_GOOGLE_OAUTH_TOKEN}"))
    );
}

#[tokio::test]
async fn test_resolve_host_credentials_missing_secret() {
    use std::collections::HashMap;

    use crate::secrets::{CredentialLocation, CredentialMapping};
    use crate::tools::wasm::capabilities::HttpCapability;
    use crate::tools::wasm::wrapper::resolve_host_credentials;

    let store = test_secrets_store();

    // No secret stored, should silently skip
    let mut credentials = HashMap::new();
    credentials.insert(
        "missing_token".to_string(),
        CredentialMapping {
            secret_name: "missing_token".to_string(),
            location: CredentialLocation::AuthorizationBearer,
            host_patterns: vec!["api.example.com".to_string()],
        },
    );

    let caps = Capabilities {
        http: Some(HttpCapability {
            credentials,
            ..Default::default()
        }),
        ..Default::default()
    };

    let result = resolve_host_credentials(&caps, Some(&store), "user1", None).await;
    assert!(result.is_empty());
}

#[tokio::test]
async fn test_resolve_host_credentials_skips_refresh_when_not_expired() {
    use std::collections::HashMap;

    use crate::secrets::{CreateSecretParams, CredentialLocation, CredentialMapping, SecretsStore};
    use crate::tools::wasm::capabilities::HttpCapability;
    use crate::tools::wasm::wrapper::{OAuthRefreshConfig, resolve_host_credentials};

    let store = test_secrets_store();

    // Store a token that expires 2 hours from now (well within buffer)
    let expires_at = chrono::Utc::now() + chrono::Duration::hours(2);
    store
        .create(
            "user1",
            CreateSecretParams::new("google_oauth_token", TEST_GOOGLE_OAUTH_FRESH)
                .with_expiry(expires_at),
        )
        .await
        .unwrap();

    let mut credentials = HashMap::new();
    credentials.insert(
        "google_oauth_token".to_string(),
        CredentialMapping {
            secret_name: "google_oauth_token".to_string(),
            location: CredentialLocation::AuthorizationBearer,
            host_patterns: vec!["www.googleapis.com".to_string()],
        },
    );

    let caps = Capabilities {
        http: Some(HttpCapability {
            credentials,
            ..Default::default()
        }),
        ..Default::default()
    };

    let oauth_config = OAuthRefreshConfig {
        token_url: "https://oauth2.googleapis.com/token".to_string(),
        client_id: TEST_OAUTH_CLIENT_ID.to_string(),
        client_secret: Some(TEST_OAUTH_CLIENT_SECRET.to_string()),
        secret_name: "google_oauth_token".to_string(),
        provider: Some("google".to_string()),
    };

    // Should resolve the existing fresh token without attempting refresh
    let result = resolve_host_credentials(&caps, Some(&store), "user1", Some(&oauth_config)).await;
    assert_eq!(result.len(), 1);
    assert_eq!(
        result[0].headers.get("Authorization"),
        Some(&format!("Bearer {TEST_GOOGLE_OAUTH_FRESH}"))
    );
}

#[tokio::test]
async fn test_resolve_host_credentials_skips_refresh_no_config() {
    use std::collections::HashMap;

    use crate::secrets::{CreateSecretParams, CredentialLocation, CredentialMapping, SecretsStore};
    use crate::tools::wasm::capabilities::HttpCapability;
    use crate::tools::wasm::wrapper::resolve_host_credentials;

    let store = test_secrets_store();

    // Store an expired token
    let expires_at = chrono::Utc::now() - chrono::Duration::hours(1);
    store
        .create(
            "user1",
            CreateSecretParams::new("my_token", "expired-value").with_expiry(expires_at),
        )
        .await
        .unwrap();

    let mut credentials = HashMap::new();
    credentials.insert(
        "my_token".to_string(),
        CredentialMapping {
            secret_name: "my_token".to_string(),
            location: CredentialLocation::AuthorizationBearer,
            host_patterns: vec!["api.example.com".to_string()],
        },
    );

    let caps = Capabilities {
        http: Some(HttpCapability {
            credentials,
            ..Default::default()
        }),
        ..Default::default()
    };

    // No OAuth config, expired token can't be resolved (get_decrypted returns Expired)
    let result = resolve_host_credentials(&caps, Some(&store), "user1", None).await;
    assert!(result.is_empty());
}

#[tokio::test]
async fn test_resolve_host_credentials_skips_refresh_no_expires_at() {
    use std::collections::HashMap;

    use crate::secrets::{CreateSecretParams, CredentialLocation, CredentialMapping, SecretsStore};
    use crate::tools::wasm::capabilities::HttpCapability;
    use crate::tools::wasm::wrapper::{OAuthRefreshConfig, resolve_host_credentials};

    let store = test_secrets_store();

    // Legacy token: no expires_at set
    store
        .create(
            "user1",
            CreateSecretParams::new("google_oauth_token", TEST_GOOGLE_OAUTH_LEGACY),
        )
        .await
        .unwrap();

    let mut credentials = HashMap::new();
    credentials.insert(
        "google_oauth_token".to_string(),
        CredentialMapping {
            secret_name: "google_oauth_token".to_string(),
            location: CredentialLocation::AuthorizationBearer,
            host_patterns: vec!["www.googleapis.com".to_string()],
        },
    );

    let caps = Capabilities {
        http: Some(HttpCapability {
            credentials,
            ..Default::default()
        }),
        ..Default::default()
    };

    let oauth_config = OAuthRefreshConfig {
        token_url: "https://oauth2.googleapis.com/token".to_string(),
        client_id: TEST_OAUTH_CLIENT_ID.to_string(),
        client_secret: Some(TEST_OAUTH_CLIENT_SECRET.to_string()),
        secret_name: "google_oauth_token".to_string(),
        provider: Some("google".to_string()),
    };

    // Should use the legacy token directly without attempting refresh
    let result = resolve_host_credentials(&caps, Some(&store), "user1", Some(&oauth_config)).await;
    assert_eq!(result.len(), 1);
    assert_eq!(
        result[0].headers.get("Authorization"),
        Some(&format!("Bearer {TEST_GOOGLE_OAUTH_LEGACY}"))
    );
}
