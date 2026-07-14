//! Unit tests for webhook secret generation and token validation.

use base64::Engine;

use super::cloudflare::validate_cloudflare_token_format;
use super::secrets::generate_webhook_secret;

#[test]
fn test_generate_webhook_secret() {
    let secret = generate_webhook_secret();
    assert_eq!(secret.len(), 64); // 32 bytes = 64 hex chars
}

#[test]
fn test_generate_secret_with_length() {
    use super::secrets::generate_secret_with_length;

    let s = generate_secret_with_length(16);
    assert_eq!(s.len(), 32); // 16 bytes = 32 hex chars
    assert!(s.chars().all(|c| c.is_ascii_hexdigit()));

    let s2 = generate_secret_with_length(1);
    assert_eq!(s2.len(), 2);
}

#[test]
fn test_validate_cloudflare_token_valid() {
    // Simulate a valid Cloudflare tunnel token: base64-encoded JSON with "a" and "t" fields
    let payload = serde_json::json!({"a": "account-tag", "t": "tunnel-id", "s": "secret"});
    let token = base64::engine::general_purpose::STANDARD.encode(payload.to_string().as_bytes());
    assert!(validate_cloudflare_token_format(&token));
}

#[test]
fn test_validate_cloudflare_token_missing_fields() {
    // JSON but missing required "a" and "t" fields
    let payload = serde_json::json!({"foo": "bar"});
    let token = base64::engine::general_purpose::STANDARD.encode(payload.to_string().as_bytes());
    assert!(!validate_cloudflare_token_format(&token));
}

#[test]
fn test_validate_cloudflare_token_not_base64() {
    assert!(!validate_cloudflare_token_format("not-base64!!!"));
}

#[test]
fn test_validate_cloudflare_token_not_json() {
    let token = base64::engine::general_purpose::STANDARD.encode(b"not json at all");
    assert!(!validate_cloudflare_token_format(&token));
}

#[test]
fn test_validate_cloudflare_token_empty() {
    assert!(!validate_cloudflare_token_format(""));
}
