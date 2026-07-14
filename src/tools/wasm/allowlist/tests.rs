//! Unit tests for the WASM HTTP endpoint allowlist validator.

use crate::tools::wasm::allowlist::{AllowlistValidator, DenyReason};
use crate::tools::wasm::capabilities::EndpointPattern;

fn validator_with_patterns() -> AllowlistValidator {
    AllowlistValidator::new(vec![
        EndpointPattern::host("api.openai.com").with_path_prefix("/v1/"),
        EndpointPattern::host("api.anthropic.com")
            .with_path_prefix("/v1/messages")
            .with_methods(vec!["POST".to_string()]),
        EndpointPattern::host("*.example.com"),
    ])
}

#[test]
fn test_allowed_request() {
    let validator = validator_with_patterns();

    let result = validator.validate("https://api.openai.com/v1/chat/completions", "POST");
    assert!(result.is_allowed());
}

#[test]
fn test_denied_wrong_host() {
    let validator = validator_with_patterns();

    let result = validator.validate("https://evil.com/steal/data", "GET");
    assert!(!result.is_allowed());

    if let super::AllowlistResult::Denied(reason) = result {
        assert!(matches!(reason, DenyReason::HostNotAllowed(_)));
    } else {
        panic!("Expected denied");
    }
}

#[test]
fn test_denied_wrong_path() {
    let validator = validator_with_patterns();

    let result = validator.validate("https://api.openai.com/v2/different", "GET");
    assert!(!result.is_allowed());

    if let super::AllowlistResult::Denied(reason) = result {
        assert!(matches!(reason, DenyReason::PathNotAllowed { .. }));
    } else {
        panic!("Expected denied");
    }
}

#[test]
fn test_denied_wrong_method() {
    let validator = validator_with_patterns();

    // Anthropic endpoint only allows POST
    let result = validator.validate("https://api.anthropic.com/v1/messages", "GET");
    assert!(!result.is_allowed());

    if let super::AllowlistResult::Denied(reason) = result {
        assert!(matches!(reason, DenyReason::MethodNotAllowed { .. }));
    } else {
        panic!("Expected denied");
    }
}

#[test]
fn test_wildcard_host() {
    let validator = validator_with_patterns();

    let result = validator.validate("https://api.example.com/anything", "GET");
    assert!(result.is_allowed());

    let result = validator.validate("https://sub.api.example.com/anything", "GET");
    assert!(result.is_allowed());
}

#[test]
fn test_require_https() {
    let validator = validator_with_patterns();

    let result = validator.validate("http://api.openai.com/v1/chat", "GET");
    assert!(!result.is_allowed());

    if let super::AllowlistResult::Denied(reason) = result {
        assert!(matches!(reason, DenyReason::InsecureScheme(_)));
    } else {
        panic!("Expected denied");
    }
}

#[test]
fn test_allow_http() {
    let validator = validator_with_patterns().allow_http();

    let result = validator.validate("http://api.example.com/test", "GET");
    assert!(result.is_allowed());
}

#[test]
fn test_empty_allowlist() {
    let validator = AllowlistValidator::new(vec![]);

    let result = validator.validate("https://anything.com/", "GET");
    assert!(!result.is_allowed());

    if let super::AllowlistResult::Denied(reason) = result {
        assert!(matches!(reason, DenyReason::EmptyAllowlist));
    } else {
        panic!("Expected denied");
    }
}

#[test]
fn test_userinfo_rejected() {
    let validator = validator_with_patterns();

    // Userinfo in URL should be rejected to prevent allowlist bypass
    let result = validator.validate("https://api.openai.com@evil.com/v1/chat", "GET");
    assert!(!result.is_allowed());

    if let super::AllowlistResult::Denied(reason) = result {
        assert!(matches!(reason, DenyReason::InvalidUrl(_)));
    } else {
        panic!("Expected denied for userinfo URL");
    }
}

#[test]
fn test_invalid_url() {
    let validator = validator_with_patterns();

    let result = validator.validate("not-a-url", "GET");
    assert!(!result.is_allowed());

    if let super::AllowlistResult::Denied(reason) = result {
        assert!(matches!(reason, DenyReason::InvalidUrl(_)));
    } else {
        panic!("Expected denied");
    }
}

#[test]
fn test_path_traversal_blocked() {
    let validator = validator_with_patterns();
    assert!(
        !validator
            .validate("https://api.openai.com/v1/../admin", "GET")
            .is_allowed()
    );
    assert!(
        !validator
            .validate("https://api.openai.com/v1/../../etc/passwd", "GET")
            .is_allowed()
    );
    assert!(
        !validator
            .validate("https://api.openai.com/v1/%2E%2E/admin", "GET")
            .is_allowed()
    );
    assert!(
        !validator
            .validate("https://api.openai.com/v1/%2e%2e/%2e%2e/root", "GET")
            .is_allowed()
    );
    assert!(
        validator
            .validate("https://api.openai.com/v1/chat/completions", "POST")
            .is_allowed()
    );
}

#[test]
fn test_normalize_path() {
    use super::normalize_path;
    assert_eq!(normalize_path("/v1/../admin").unwrap(), "/admin");
    assert_eq!(
        normalize_path("/v1/chat/completions").unwrap(),
        "/v1/chat/completions"
    );
    assert_eq!(normalize_path("/v1/./chat").unwrap(), "/v1/chat");
    assert_eq!(
        normalize_path("/v1/../../../etc/passwd").unwrap(),
        "/etc/passwd"
    );
    assert_eq!(normalize_path("/v1/%2e%2e/admin").unwrap(), "/admin");
    assert_eq!(normalize_path("/").unwrap(), "/");
    assert_eq!(normalize_path("/v1/").unwrap(), "/v1/");
}

#[test]
fn test_invalid_encoded_path_rejected() {
    let validator = validator_with_patterns();
    let result = validator.validate("https://api.openai.com/v1/%ZZ/chat", "GET");
    assert!(!result.is_allowed());
    if let super::AllowlistResult::Denied(reason) = result {
        assert!(matches!(reason, DenyReason::InvalidUrl(_)));
    } else {
        panic!("Expected denied");
    }
}

#[test]
fn test_encoded_separator_rejected() {
    let validator = validator_with_patterns();
    let result = validator.validate("https://api.openai.com/v1/%2Fadmin", "GET");
    assert!(!result.is_allowed());
    if let super::AllowlistResult::Denied(reason) = result {
        assert!(matches!(reason, DenyReason::InvalidUrl(_)));
    } else {
        panic!("Expected denied");
    }
}

#[test]
fn test_percent_encoding_validator() {
    use super::has_valid_percent_encoding;
    assert!(has_valid_percent_encoding("%2F"));
    assert!(has_valid_percent_encoding("hello%20world"));
    assert!(!has_valid_percent_encoding("%"));
    assert!(!has_valid_percent_encoding("%2"));
    assert!(!has_valid_percent_encoding("%ZZ"));
}

#[test]
fn test_url_with_port() {
    let validator = AllowlistValidator::new(vec![EndpointPattern::host("localhost")]).allow_http();

    let result = validator.validate("http://localhost:8080/api", "GET");
    assert!(result.is_allowed());
}

#[test]
fn test_reject_url_with_userinfo() {
    let validator = validator_with_patterns();

    // Attacker uses userinfo to trick the parser: the allowlist sees
    // "api.openai.com" but reqwest would actually connect to "evil.com".
    let result = validator.validate("https://api.openai.com@evil.com/v1/steal", "GET");
    assert!(!result.is_allowed());

    if let super::AllowlistResult::Denied(reason) = result {
        assert!(matches!(reason, DenyReason::InvalidUrl(_)));
    } else {
        panic!("Expected denied due to userinfo");
    }
}

#[test]
fn test_reject_url_with_user_pass() {
    let validator = validator_with_patterns();

    let result = validator.validate("https://user:password@api.openai.com/v1/chat", "GET");
    assert!(!result.is_allowed());
}
