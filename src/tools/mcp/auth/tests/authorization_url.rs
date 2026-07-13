//! Tests for PKCE challenge generation and authorization URL construction.

use std::collections::HashMap;

use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use sha2::{Digest, Sha256};

use super::super::{PkceChallenge, build_authorization_url};

#[test]
fn test_pkce_challenge_generation() {
    let pkce = PkceChallenge::generate();

    // Verifier should be base64url encoded
    assert!(!pkce.verifier.is_empty());
    assert!(!pkce.verifier.contains('+'));
    assert!(!pkce.verifier.contains('/'));
    assert!(!pkce.verifier.contains('='));

    // Challenge should be different from verifier
    assert_ne!(pkce.verifier, pkce.challenge);

    // Two challenges should be different
    let pkce2 = PkceChallenge::generate();
    assert_ne!(pkce.verifier, pkce2.verifier);
}

#[test]
fn test_build_authorization_url() {
    let url = build_authorization_url(
        "https://auth.example.com/authorize",
        "client-123",
        "http://localhost:9876/callback",
        &["read".to_string(), "write".to_string()],
        None,
        &HashMap::new(),
        None,
    );

    assert!(url.starts_with("https://auth.example.com/authorize?"));
    assert!(url.contains("client_id=client-123"));
    assert!(url.contains("response_type=code"));
    assert!(url.contains("redirect_uri="));
    assert!(url.contains("scope=read%20write"));
}

#[test]
fn test_build_authorization_url_with_pkce() {
    let pkce = PkceChallenge::generate();
    let url = build_authorization_url(
        "https://auth.example.com/authorize",
        "client-123",
        "http://localhost:9876/callback",
        &[],
        Some(&pkce),
        &HashMap::new(),
        None,
    );

    assert!(url.contains(&format!("code_challenge={}", pkce.challenge)));
    assert!(url.contains("code_challenge_method=S256"));
}

#[test]
fn test_build_authorization_url_with_extra_params() {
    let mut extra = HashMap::new();
    extra.insert("owner".to_string(), "user".to_string());
    extra.insert("state".to_string(), "abc123".to_string());

    let url = build_authorization_url(
        "https://auth.example.com/authorize",
        "client-123",
        "http://localhost:9876/callback",
        &[],
        None,
        &extra,
        None,
    );

    assert!(url.contains("owner=user"));
    assert!(url.contains("state=abc123"));
}

#[test]
fn test_pkce_challenge_s256_is_correct_sha256() {
    let pkce = PkceChallenge::generate();

    // Recompute the S256 challenge from scratch and compare.
    let mut hasher = Sha256::new();
    hasher.update(pkce.verifier.as_bytes());
    let expected = URL_SAFE_NO_PAD.encode(hasher.finalize());

    assert_eq!(pkce.challenge, expected);
}

#[test]
fn test_build_authorization_url_empty_scopes_no_scope_param() {
    let url = build_authorization_url(
        "https://auth.example.com/authorize",
        "client-123",
        "http://localhost:9876/callback",
        &[],
        None,
        &HashMap::new(),
        None,
    );

    // With no scopes, the URL must not contain a scope parameter at all.
    assert!(!url.contains("scope="));
}

#[test]
fn test_build_authorization_url_special_characters_are_encoded() {
    let url = build_authorization_url(
        "https://auth.example.com/authorize",
        "client id&evil=true",
        "http://localhost:9876/call back?x=1",
        &[],
        None,
        &HashMap::new(),
        None,
    );

    // Spaces and ampersands in client_id must be percent-encoded.
    assert!(url.contains("client_id=client%20id%26evil%3Dtrue"));
    // Spaces and question marks in redirect_uri must be percent-encoded.
    assert!(url.contains("redirect_uri=http%3A%2F%2Flocalhost%3A9876%2Fcall%20back%3Fx%3D1"));
}

#[test]
fn test_build_authorization_url_with_resource() {
    let url = build_authorization_url(
        "https://auth.example.com/authorize",
        "client-123",
        "http://localhost:9876/callback",
        &[],
        None,
        &HashMap::new(),
        Some("https://mcp.example.com/v1"),
    );

    assert!(url.contains("resource=https%3A%2F%2Fmcp.example.com%2Fv1"));
}

#[test]
fn test_build_authorization_url_without_resource() {
    let url = build_authorization_url(
        "https://auth.example.com/authorize",
        "client-123",
        "http://localhost:9876/callback",
        &[],
        None,
        &HashMap::new(),
        None,
    );

    assert!(!url.contains("resource="));
}
