//! Tests for URL sanitization used in extension logging.

use crate::extensions::manager::sanitize_url_for_logging;

#[test]
fn test_sanitize_url_with_query_params() {
    let url = "https://api.example.com/path?api_key=secret123&token=abc";
    let result = sanitize_url_for_logging(url);
    assert_eq!(result, "https://api.example.com/path");
    assert!(!result.contains("api_key"));
    assert!(!result.contains("secret123"));
    assert!(!result.contains("token"));
}

#[test]
fn test_sanitize_url_with_credentials() {
    let url = "https://user:password@api.example.com:8080/path";
    let result = sanitize_url_for_logging(url);
    assert!(!result.contains("user"));
    assert!(!result.contains("password"));
    assert!(!result.contains("@"));
    assert!(result.contains("api.example.com"));
    assert!(result.contains(":8080"));
}

#[test]
fn test_sanitize_url_with_fragment() {
    let url = "https://api.example.com/path#section";
    let result = sanitize_url_for_logging(url);
    assert_eq!(result, "https://api.example.com/path");
    assert!(!result.contains("#"));
    assert!(!result.contains("section"));
}

#[test]
fn test_sanitize_url_with_port() {
    let url = "https://api.example.com:9443/path?key=value";
    let result = sanitize_url_for_logging(url);
    assert_eq!(result, "https://api.example.com:9443/path");
    assert!(result.contains(":9443"));
    assert!(!result.contains("key"));
}

#[test]
fn test_sanitize_url_with_all_components() {
    let url = "https://admin:secret@api.example.com:8080/v1/data?api_key=xyz#results";
    let result = sanitize_url_for_logging(url);
    assert!(!result.contains("admin"));
    assert!(!result.contains("secret"));
    assert!(!result.contains("@"));
    assert!(!result.contains("api_key"));
    assert!(!result.contains("xyz"));
    assert!(!result.contains("#"));
    assert!(!result.contains("results"));
    assert!(result.contains("api.example.com:8080"));
    assert!(result.contains("/v1/data"));
}

#[test]
fn test_sanitize_url_malformed() {
    // Malformed URL should fallback to string splitting
    let url = "https://[invalid-url";
    let result = sanitize_url_for_logging(url);
    // Malformed URL without query should return as-is via fallback
    assert_eq!(result, url);

    // Should still strip query params via fallback
    let url_with_query = "https://[invalid-url?key=secret";
    let result_with_query = sanitize_url_for_logging(url_with_query);
    assert_eq!(result_with_query, "https://[invalid-url");
    assert!(!result_with_query.contains("?"));
    assert!(!result_with_query.contains("secret"));
}

#[test]
fn test_sanitize_url_short_string() {
    let url = "short";
    let result = sanitize_url_for_logging(url);
    assert_eq!(result, "short");
}

#[test]
fn test_sanitize_url_not_url_like() {
    let input = "this is not a url";
    let result = sanitize_url_for_logging(input);
    assert_eq!(result, input);
}

#[test]
fn test_sanitize_url_preserves_path() {
    let url = "https://api.example.com/v1/users/123/profile";
    let result = sanitize_url_for_logging(url);
    assert_eq!(result, url);
    assert!(result.contains("/v1/users/123/profile"));
}
