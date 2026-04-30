//! Unit tests for WASM HTTP credential injection and shared registry behaviour.

use std::collections::HashMap;

use crate::secrets::{
    CreateSecretParams, CredentialLocation, CredentialMapping, InMemorySecretsStore, SecretsStore,
};
use crate::testing::credentials::{TEST_OPENAI_API_KEY, test_secrets_store};
use crate::tools::wasm::SharedCredentialRegistry;
use crate::tools::wasm::credential_injector::{
    CredentialInjector, base64_encode, host_matches_pattern,
};

fn test_store() -> InMemorySecretsStore {
    test_secrets_store()
}

#[test]
fn test_host_matches_exact() {
    assert!(host_matches_pattern("api.openai.com", "api.openai.com"));
    assert!(!host_matches_pattern("api.openai.com", "other.com"));
}

#[test]
fn test_host_matches_wildcard() {
    assert!(host_matches_pattern("api.example.com", "*.example.com"));
    assert!(host_matches_pattern("sub.api.example.com", "*.example.com"));
    assert!(!host_matches_pattern("example.com", "*.example.com"));
}

#[test]
fn test_base64_encode() {
    assert_eq!(base64_encode(b"hello"), "aGVsbG8=");
    assert_eq!(base64_encode(b"user:pass"), "dXNlcjpwYXNz");
}

#[tokio::test]
async fn test_inject_bearer() {
    let store = test_store();
    store
        .create(
            "user1",
            CreateSecretParams::new("openai_key", TEST_OPENAI_API_KEY),
        )
        .await
        .unwrap();

    let mut mappings = HashMap::new();
    mappings.insert(
        "openai".to_string(),
        CredentialMapping {
            secret_name: "openai_key".to_string(),
            location: CredentialLocation::AuthorizationBearer,
            host_patterns: vec!["api.openai.com".to_string()],
        },
    );

    let injector = CredentialInjector::new(mappings, vec!["openai_key".to_string()]);
    let result = injector
        .inject("user1", "api.openai.com", &store)
        .await
        .unwrap();

    assert_eq!(
        result.headers.get("Authorization"),
        Some(&format!("Bearer {TEST_OPENAI_API_KEY}"))
    );
}

#[tokio::test]
async fn test_inject_custom_header() {
    let store = test_store();
    store
        .create("user1", CreateSecretParams::new("api_key", "secret123"))
        .await
        .unwrap();

    let mut mappings = HashMap::new();
    mappings.insert(
        "custom".to_string(),
        CredentialMapping {
            secret_name: "api_key".to_string(),
            location: CredentialLocation::Header {
                name: "X-API-Key".to_string(),
                prefix: None,
            },
            host_patterns: vec!["*.example.com".to_string()],
        },
    );

    let injector = CredentialInjector::new(mappings, vec!["api_key".to_string()]);
    let result = injector
        .inject("user1", "api.example.com", &store)
        .await
        .unwrap();

    assert_eq!(
        result.headers.get("X-API-Key"),
        Some(&"secret123".to_string())
    );
}

#[tokio::test]
async fn test_inject_basic_auth() {
    let store = test_store();
    store
        .create("user1", CreateSecretParams::new("password", "mypassword"))
        .await
        .unwrap();

    let mut mappings = HashMap::new();
    mappings.insert(
        "basic".to_string(),
        CredentialMapping {
            secret_name: "password".to_string(),
            location: CredentialLocation::AuthorizationBasic {
                username: "myuser".to_string(),
            },
            host_patterns: vec!["api.service.com".to_string()],
        },
    );

    let injector = CredentialInjector::new(mappings, vec!["password".to_string()]);
    let result = injector
        .inject("user1", "api.service.com", &store)
        .await
        .unwrap();

    let expected = format!("Basic {}", base64_encode(b"myuser:mypassword"));
    assert_eq!(result.headers.get("Authorization"), Some(&expected));
}

#[tokio::test]
async fn test_no_credentials_for_host() {
    let store = test_store();

    let injector = CredentialInjector::new(HashMap::new(), vec![]);
    let result = injector
        .inject("user1", "unknown.com", &store)
        .await
        .unwrap();

    assert!(result.is_empty());
}

#[tokio::test]
async fn test_access_denied_for_secret() {
    let store = test_store();
    store
        .create("user1", CreateSecretParams::new("secret_key", "value"))
        .await
        .unwrap();

    let mut mappings = HashMap::new();
    mappings.insert(
        "test".to_string(),
        CredentialMapping {
            secret_name: "secret_key".to_string(),
            location: CredentialLocation::AuthorizationBearer,
            host_patterns: vec!["api.test.com".to_string()],
        },
    );

    let injector = CredentialInjector::new(mappings, vec![]);
    let result = injector.inject("user1", "api.test.com", &store).await;

    assert!(result.is_err());
}

#[test]
fn test_shared_registry_empty() {
    let registry = SharedCredentialRegistry::new();
    assert!(!registry.has_credentials_for_host("api.example.com"));
    assert!(registry.find_for_host("api.example.com").is_empty());
}

#[test]
fn test_shared_registry_add_and_find() {
    let registry = SharedCredentialRegistry::new();
    registry.add_mappings(vec![
        CredentialMapping::bearer("openai_key", "api.openai.com"),
        CredentialMapping::header("github_token", "X-GitHub-Token", "*.github.com"),
    ]);

    assert!(registry.has_credentials_for_host("api.openai.com"));
    assert!(!registry.has_credentials_for_host("api.anthropic.com"));

    let found = registry.find_for_host("api.openai.com");
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].secret_name, "openai_key");
}

#[test]
fn test_shared_registry_wildcard_host() {
    let registry = SharedCredentialRegistry::new();
    registry.add_mappings(vec![CredentialMapping::bearer("gh_token", "*.github.com")]);

    assert!(registry.has_credentials_for_host("api.github.com"));
    assert!(registry.has_credentials_for_host("uploads.github.com"));
    assert!(!registry.has_credentials_for_host("github.com"));
}

#[test]
fn test_shared_registry_multiple_adds() {
    let registry = SharedCredentialRegistry::new();
    registry.add_mappings(vec![CredentialMapping::bearer("key1", "api.example.com")]);
    registry.add_mappings(vec![CredentialMapping::bearer("key2", "api.example.com")]);

    let found = registry.find_for_host("api.example.com");
    assert_eq!(found.len(), 2);
}

#[test]
fn test_shared_registry_merges_anonymous_mappings_with_same_secret_name() {
    let registry = SharedCredentialRegistry::new();
    registry.add_mappings(vec![CredentialMapping::bearer("key1", "old.example.com")]);
    registry.add_mappings(vec![CredentialMapping::bearer("key1", "api.example.com")]);

    assert_eq!(registry.find_for_host("old.example.com").len(), 1);
    let found = registry.find_for_host("api.example.com");
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].secret_name, "key1");
}

#[test]
fn test_shared_registry_replaces_mappings_for_same_tool_only() {
    let registry = SharedCredentialRegistry::new();
    registry.add_mappings_for_tool(
        "tool_a",
        vec![CredentialMapping::bearer("key1", "old.example.com")],
    );
    registry.add_mappings_for_tool(
        "tool_b",
        vec![CredentialMapping::bearer("key1", "other.example.com")],
    );
    registry.add_mappings_for_tool(
        "tool_a",
        vec![CredentialMapping::bearer("key1", "api.example.com")],
    );

    assert!(registry.find_for_host("old.example.com").is_empty());
    assert_eq!(registry.find_for_host("api.example.com").len(), 1);
    assert_eq!(registry.find_for_host("other.example.com").len(), 1);
}

#[test]
fn test_shared_registry_remove_mappings_for_secrets() {
    let registry = SharedCredentialRegistry::new();
    registry.add_mappings(vec![
        CredentialMapping::bearer("openai_key", "api.openai.com"),
        CredentialMapping::bearer("gh_token", "*.github.com"),
        CredentialMapping::header("openai_org", "OpenAI-Organization", "api.openai.com"),
    ]);

    assert_eq!(registry.find_for_host("api.openai.com").len(), 2);
    assert!(registry.has_credentials_for_host("api.github.com"));

    registry.remove_mappings_for_secrets(&["openai_key".to_string(), "openai_org".to_string()]);

    assert!(registry.find_for_host("api.openai.com").is_empty());
    assert!(registry.has_credentials_for_host("api.github.com"));
}

#[test]
fn test_shared_registry_remove_nonexistent_is_noop() {
    let registry = SharedCredentialRegistry::new();
    registry.add_mappings(vec![CredentialMapping::bearer("key1", "api.example.com")]);

    registry.remove_mappings_for_secrets(&["nonexistent".to_string()]);
    assert_eq!(registry.find_for_host("api.example.com").len(), 1);
}

#[test]
fn test_shared_registry_thread_safety() {
    use std::sync::Arc;
    use std::thread;

    let registry = Arc::new(SharedCredentialRegistry::new());

    let handles: Vec<_> = (0..4)
        .map(|i| {
            let r = Arc::clone(&registry);
            thread::spawn(move || {
                r.add_mappings(vec![CredentialMapping::bearer(
                    format!("key_{i}"),
                    "api.example.com",
                )]);
            })
        })
        .collect();

    for h in handles {
        h.join().unwrap();
    }

    let found = registry.find_for_host("api.example.com");
    assert_eq!(found.len(), 4);
}
