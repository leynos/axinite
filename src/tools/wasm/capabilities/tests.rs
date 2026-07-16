//! Unit tests for WASM capability defaults and endpoint pattern
//! matching.

use crate::tools::wasm::capabilities::{Capabilities, EndpointPattern, SecretsCapability};

#[test]
fn test_capabilities_default_is_none() {
    let caps = Capabilities::default();
    assert!(caps.workspace_read.is_none());
    assert!(caps.http.is_none());
    assert!(caps.tool_invoke.is_none());
    assert!(caps.secrets.is_none());
}

#[test]
fn test_endpoint_pattern_exact_host() {
    let pattern = EndpointPattern::host("api.example.com");

    assert!(pattern.matches("api.example.com", "/", "GET"));
    assert!(!pattern.matches("other.example.com", "/", "GET"));
}

#[test]
fn test_endpoint_pattern_wildcard_host() {
    let pattern = EndpointPattern::host("*.example.com");

    assert!(pattern.matches("api.example.com", "/", "GET"));
    assert!(pattern.matches("sub.api.example.com", "/", "GET"));
    assert!(!pattern.matches("example.com", "/", "GET"));
    assert!(!pattern.matches("notexample.com", "/", "GET"));
}

#[test]
fn test_endpoint_pattern_path_prefix() {
    let pattern = EndpointPattern::host("api.example.com").with_path_prefix("/v1/");

    assert!(pattern.matches("api.example.com", "/v1/users", "GET"));
    assert!(pattern.matches("api.example.com", "/v1/", "GET"));
    assert!(!pattern.matches("api.example.com", "/v2/users", "GET"));
    assert!(!pattern.matches("api.example.com", "/", "GET"));
}

#[test]
fn test_endpoint_pattern_methods() {
    let pattern = EndpointPattern::host("api.example.com")
        .with_methods(vec!["GET".to_string(), "POST".to_string()]);

    assert!(pattern.matches("api.example.com", "/", "GET"));
    assert!(pattern.matches("api.example.com", "/", "get")); // case insensitive
    assert!(pattern.matches("api.example.com", "/", "POST"));
    assert!(!pattern.matches("api.example.com", "/", "DELETE"));
}

#[test]
fn test_secrets_capability_exact_match() {
    let cap = SecretsCapability {
        allowed_names: vec!["openai_key".to_string()],
    };

    assert!(cap.is_allowed("openai_key"));
    assert!(!cap.is_allowed("anthropic_key"));
}

#[test]
fn test_secrets_capability_glob() {
    let cap = SecretsCapability {
        allowed_names: vec!["openai_*".to_string()],
    };

    assert!(cap.is_allowed("openai_key"));
    assert!(cap.is_allowed("openai_org"));
    assert!(!cap.is_allowed("anthropic_key"));
}

#[test]
fn test_capabilities_builder() {
    let caps = Capabilities::none()
        .with_workspace_read(vec!["context/".to_string()])
        .with_secrets(vec!["test_*".to_string()]);

    assert!(caps.workspace_read.is_some());
    assert!(caps.secrets.is_some());
    assert!(caps.http.is_none());
}
