//! Tests for well-known URI construction, canonical resource URIs, SSRF
//! protection, and WWW-Authenticate header parsing.

use std::net::IpAddr;

use super::super::discovery::parse_resource_metadata_url;
use super::super::url_safety::{is_dangerous_ip, validate_url_safe};
use super::super::{build_well_known_uri, canonical_resource_uri};

// --- Well-known URI construction ---

#[test]
fn test_build_well_known_uri_no_path() {
    let uri = build_well_known_uri("https://example.com", "oauth-authorization-server").unwrap();
    assert_eq!(
        uri,
        "https://example.com/.well-known/oauth-authorization-server"
    );
}

#[test]
fn test_build_well_known_uri_with_path() {
    let uri =
        build_well_known_uri("https://example.com/path", "oauth-authorization-server").unwrap();
    assert_eq!(
        uri,
        "https://example.com/.well-known/oauth-authorization-server/path"
    );
}

#[test]
fn test_build_well_known_uri_with_trailing_slash() {
    let uri =
        build_well_known_uri("https://example.com/path/", "oauth-protected-resource").unwrap();
    assert_eq!(
        uri,
        "https://example.com/.well-known/oauth-protected-resource/path"
    );
}

#[test]
fn test_build_well_known_uri_root_trailing_slash() {
    let uri = build_well_known_uri("https://example.com/", "oauth-authorization-server").unwrap();
    assert_eq!(
        uri,
        "https://example.com/.well-known/oauth-authorization-server"
    );
}

// --- canonical_resource_uri ---

#[test]
fn test_canonical_resource_uri_strips_fragment() {
    assert_eq!(
        canonical_resource_uri("https://mcp.example.com/v1#section"),
        "https://mcp.example.com/v1"
    );
}

#[test]
fn test_canonical_resource_uri_strips_trailing_slash() {
    assert_eq!(
        canonical_resource_uri("https://mcp.example.com/v1/"),
        "https://mcp.example.com/v1"
    );
}

#[test]
fn test_canonical_resource_uri_no_changes_needed() {
    assert_eq!(
        canonical_resource_uri("https://mcp.example.com/v1"),
        "https://mcp.example.com/v1"
    );
}

// --- SSRF protection ---

#[test]
fn test_is_dangerous_ip_loopback_v4() {
    assert!(is_dangerous_ip("127.0.0.1".parse().unwrap()));
    assert!(is_dangerous_ip("127.0.0.2".parse().unwrap()));
}

#[test]
fn test_is_dangerous_ip_private_v4() {
    assert!(is_dangerous_ip("10.0.0.1".parse().unwrap()));
    assert!(is_dangerous_ip("172.16.0.1".parse().unwrap()));
    assert!(is_dangerous_ip("192.168.1.1".parse().unwrap()));
}

#[test]
fn test_is_dangerous_ip_link_local_v4() {
    assert!(is_dangerous_ip("169.254.169.254".parse().unwrap()));
}

#[test]
fn test_is_dangerous_ip_cgnat() {
    assert!(is_dangerous_ip("100.64.0.1".parse().unwrap()));
    assert!(is_dangerous_ip("100.127.255.254".parse().unwrap()));
}

#[test]
fn test_is_dangerous_ip_safe_v4() {
    assert!(!is_dangerous_ip("8.8.8.8".parse().unwrap()));
    assert!(!is_dangerous_ip("1.1.1.1".parse().unwrap()));
}

#[test]
fn test_is_dangerous_ip_ipv4_mapped_v6_loopback() {
    // ::ffff:127.0.0.1 must be blocked
    let ip: IpAddr = "::ffff:127.0.0.1".parse().unwrap();
    assert!(is_dangerous_ip(ip));
}

#[test]
fn test_is_dangerous_ip_ipv4_mapped_v6_link_local() {
    // ::ffff:169.254.169.254 must be blocked
    let ip: IpAddr = "::ffff:169.254.169.254".parse().unwrap();
    assert!(is_dangerous_ip(ip));
}

#[test]
fn test_is_dangerous_ip_unspecified() {
    assert!(is_dangerous_ip("0.0.0.0".parse().unwrap()));
    assert!(is_dangerous_ip("::".parse().unwrap()));
}

#[test]
fn test_is_dangerous_ip_v6_loopback() {
    assert!(is_dangerous_ip("::1".parse().unwrap()));
}

#[tokio::test]
async fn test_validate_url_safe_https() {
    assert!(validate_url_safe("https://example.com/path").await.is_ok());
}

#[tokio::test]
async fn test_validate_url_safe_http_localhost_allowed() {
    // HTTP is only allowed for localhost dev scenarios
    assert!(validate_url_safe("http://localhost/path").await.is_ok());
    assert!(
        validate_url_safe("http://localhost:8080/path")
            .await
            .is_ok()
    );
}

#[tokio::test]
async fn test_validate_url_safe_http_non_localhost_rejected() {
    // HTTP to non-localhost hosts must be rejected (plaintext credential risk)
    assert!(validate_url_safe("http://example.com/path").await.is_err());
}

#[tokio::test]
async fn test_validate_url_safe_bad_scheme() {
    assert!(validate_url_safe("ftp://example.com/path").await.is_err());
    assert!(validate_url_safe("file:///etc/passwd").await.is_err());
}

#[tokio::test]
async fn test_validate_url_safe_private_ip() {
    // 127.0.0.1 over HTTP is allowed (localhost dev scenario)
    assert!(validate_url_safe("http://127.0.0.1/path").await.is_ok());
    // Private/link-local IPs over HTTPS are blocked (SSRF protection)
    assert!(validate_url_safe("https://10.0.0.1/path").await.is_err());
    assert!(
        validate_url_safe("https://169.254.169.254/latest/meta-data")
            .await
            .is_err()
    );
    // Private IPs over HTTP (non-localhost) are blocked
    assert!(validate_url_safe("http://10.0.0.1/path").await.is_err());
}

#[tokio::test]
async fn test_validate_url_safe_public_ip() {
    assert!(validate_url_safe("https://8.8.8.8/dns").await.is_ok());
}

// --- parse_resource_metadata_url ---

#[test]
fn test_parse_resource_metadata_url_bearer() {
    let header = r#"Bearer resource_metadata="https://res.example.com/.well-known/oauth-protected-resource""#;
    let url = parse_resource_metadata_url(header);
    assert_eq!(
        url.as_deref(),
        Some("https://res.example.com/.well-known/oauth-protected-resource")
    );
}

#[test]
fn test_parse_resource_metadata_url_with_other_params() {
    let header = r#"Bearer realm="example", resource_metadata="https://res.example.com/meta""#;
    let url = parse_resource_metadata_url(header);
    assert_eq!(url.as_deref(), Some("https://res.example.com/meta"));
}

#[test]
fn test_parse_resource_metadata_url_missing() {
    let header = r#"Bearer realm="example""#;
    let url = parse_resource_metadata_url(header);
    assert!(url.is_none());
}
