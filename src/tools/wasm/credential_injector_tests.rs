//! Unit tests for WASM HTTP credential injection and shared registry behaviour.

use std::collections::HashMap;

use rstest::fixture;
use rstest::rstest;

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

#[rstest]
#[case("api.openai.com", "api.openai.com", true)]
#[case("api.openai.com", "other.com", false)]
#[case("api.example.com", "*.example.com", true)]
#[case("sub.api.example.com", "*.example.com", true)]
#[case("example.com", "*.example.com", false)]
fn test_host_matches_cases(#[case] host: &str, #[case] pattern: &str, #[case] expected: bool) {
    assert_eq!(host_matches_pattern(host, pattern), expected);
}

#[test]
fn test_base64_encode() {
    assert_eq!(base64_encode(b"hello"), "aGVsbG8=");
    assert_eq!(base64_encode(b"user:pass"), "dXNlcjpwYXNz");
}

struct InjectionSpec<'a> {
    secret_name: &'a str,
    secret_value: &'a str,
    mapping_key: &'a str,
    location: CredentialLocation,
    host_pattern: &'a str,
    target_host: &'a str,
}

async fn run_single_mapping_injection(spec: InjectionSpec<'_>) -> HashMap<String, String> {
    let store = test_store();
    store
        .create(
            "user1",
            CreateSecretParams::new(spec.secret_name, spec.secret_value),
        )
        .await
        .unwrap();

    let mut mappings = HashMap::new();
    mappings.insert(
        spec.mapping_key.to_string(),
        CredentialMapping {
            secret_name: spec.secret_name.to_string(),
            location: spec.location,
            host_patterns: vec![spec.host_pattern.to_string()],
        },
    );

    let injector = CredentialInjector::new(mappings, vec![spec.secret_name.to_string()]);
    injector
        .inject("user1", spec.target_host, &store)
        .await
        .unwrap()
        .headers
}

#[tokio::test]
async fn test_inject_bearer() {
    let headers = run_single_mapping_injection(InjectionSpec {
        secret_name: "openai_key",
        secret_value: TEST_OPENAI_API_KEY,
        mapping_key: "openai",
        location: CredentialLocation::AuthorizationBearer,
        host_pattern: "api.openai.com",
        target_host: "api.openai.com",
    })
    .await;
    assert_eq!(
        headers.get("Authorization"),
        Some(&format!("Bearer {TEST_OPENAI_API_KEY}"))
    );
}

#[tokio::test]
async fn test_inject_custom_header() {
    let headers = run_single_mapping_injection(InjectionSpec {
        secret_name: "api_key",
        secret_value: "secret123",
        mapping_key: "custom",
        location: CredentialLocation::Header {
            name: "X-API-Key".to_string(),
            prefix: None,
        },
        host_pattern: "*.example.com",
        target_host: "api.example.com",
    })
    .await;
    assert_eq!(headers.get("X-API-Key"), Some(&"secret123".to_string()));
}

#[tokio::test]
async fn test_inject_basic_auth() {
    let headers = run_single_mapping_injection(InjectionSpec {
        secret_name: "password",
        secret_value: "mypassword",
        mapping_key: "basic",
        location: CredentialLocation::AuthorizationBasic {
            username: "myuser".to_string(),
        },
        host_pattern: "api.service.com",
        target_host: "api.service.com",
    })
    .await;
    let expected = format!("Basic {}", base64_encode(b"myuser:mypassword"));
    assert_eq!(headers.get("Authorization"), Some(&expected));
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

#[fixture]
fn registry() -> SharedCredentialRegistry {
    SharedCredentialRegistry::new()
}

#[rstest]
fn test_shared_registry_empty(registry: SharedCredentialRegistry) {
    assert!(!registry.has_credentials_for_host("api.example.com"));
    assert!(registry.find_for_host("api.example.com").is_empty());
}

#[rstest]
fn test_shared_registry_add_and_find(registry: SharedCredentialRegistry) {
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

#[rstest]
fn test_shared_registry_wildcard_host(registry: SharedCredentialRegistry) {
    registry.add_mappings(vec![CredentialMapping::bearer("gh_token", "*.github.com")]);

    assert!(registry.has_credentials_for_host("api.github.com"));
    assert!(registry.has_credentials_for_host("uploads.github.com"));
    assert!(!registry.has_credentials_for_host("github.com"));
}

#[rstest]
fn test_shared_registry_multiple_adds(registry: SharedCredentialRegistry) {
    registry.add_mappings(vec![CredentialMapping::bearer("key1", "api.example.com")]);
    registry.add_mappings(vec![CredentialMapping::bearer("key2", "api.example.com")]);

    let found = registry.find_for_host("api.example.com");
    assert_eq!(found.len(), 2);
}

#[rstest]
fn test_shared_registry_merges_anonymous_mappings_with_same_secret_name(
    registry: SharedCredentialRegistry,
) {
    registry.add_mappings(vec![CredentialMapping::bearer("key1", "old.example.com")]);
    registry.add_mappings(vec![CredentialMapping::bearer("key1", "api.example.com")]);

    assert_eq!(registry.find_for_host("old.example.com").len(), 1);
    let found = registry.find_for_host("api.example.com");
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].secret_name, "key1");
}

#[rstest]
fn test_shared_registry_replaces_mappings_for_same_tool_only(registry: SharedCredentialRegistry) {
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

#[rstest]
fn test_shared_registry_remove_mappings_for_secrets(registry: SharedCredentialRegistry) {
    registry.add_mappings_for_tool(
        "test_tool",
        vec![
            CredentialMapping::bearer("openai_key", "api.openai.com"),
            CredentialMapping::bearer("gh_token", "*.github.com"),
            CredentialMapping::header("openai_org", "OpenAI-Organization", "api.openai.com"),
        ],
    );

    assert_eq!(registry.find_for_host("api.openai.com").len(), 2);
    assert!(registry.has_credentials_for_host("api.github.com"));

    registry.remove_mappings_for_secrets(
        "test_tool",
        &["openai_key".to_string(), "openai_org".to_string()],
    );

    assert!(registry.find_for_host("api.openai.com").is_empty());
    assert!(registry.has_credentials_for_host("api.github.com"));
}

#[rstest]
fn test_shared_registry_remove_nonexistent_is_noop(registry: SharedCredentialRegistry) {
    registry.add_mappings_for_tool(
        "test_tool",
        vec![CredentialMapping::bearer("key1", "api.example.com")],
    );

    registry.remove_mappings_for_secrets("test_tool", &["nonexistent".to_string()]);
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
