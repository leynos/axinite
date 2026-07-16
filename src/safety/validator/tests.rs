//! Unit tests for input safety validation.

use super::*;

#[test]
fn test_valid_input() {
    let validator = Validator::new();
    let result = validator.validate("Hello, this is a normal message.");
    assert!(result.is_valid);
    assert!(result.errors.is_empty());
}

#[test]
fn test_empty_input() {
    let validator = Validator::new();
    let result = validator.validate("");
    assert!(!result.is_valid);
    assert!(
        result
            .errors
            .iter()
            .any(|e| e.code == ValidationErrorCode::Empty)
    );
}

#[test]
fn test_too_long_input() {
    let validator = Validator::new().with_max_length(10);
    let result = validator.validate("This is way too long for the limit");
    assert!(!result.is_valid);
    assert!(
        result
            .errors
            .iter()
            .any(|e| e.code == ValidationErrorCode::TooLong)
    );
}

#[test]
fn test_forbidden_pattern() {
    let validator = Validator::new().forbid_pattern("forbidden");
    let result = validator.validate("This contains FORBIDDEN content");
    assert!(!result.is_valid);
    assert!(
        result
            .errors
            .iter()
            .any(|e| e.code == ValidationErrorCode::ForbiddenContent)
    );
}

#[test]
fn test_excessive_repetition_warning() {
    let validator = Validator::new();
    // String needs to be >= 50 chars for repetition check
    let result = validator.validate(&format!("Start of message{}End of message", "a".repeat(30)));
    assert!(result.is_valid); // Still valid, just a warning
    assert!(!result.warnings.is_empty());
}

#[test]
fn test_tool_params_allow_empty_strings() {
    let validator = Validator::new();
    let result = validator.validate_tool_params(&serde_json::json!({
        "path": "",
        "nested": {
            "label": ""
        },
        "items": [""]
    }));

    assert!(result.is_valid);
    assert!(result.errors.is_empty());
}

#[test]
fn test_tool_params_still_block_null_bytes() {
    let validator = Validator::new();
    let result = validator.validate_tool_params(&serde_json::json!({
        "path": "bad\u{0000}path"
    }));

    assert!(!result.is_valid);
    assert!(
        result
            .errors
            .iter()
            .any(|e| e.code == ValidationErrorCode::InvalidEncoding)
    );
}

#[test]
fn test_tool_params_still_block_forbidden_patterns() {
    let validator = Validator::new().forbid_pattern("forbidden");
    let result = validator.validate_tool_params(&serde_json::json!({
        "path": "contains forbidden content"
    }));

    assert!(!result.is_valid);
    assert!(
        result
            .errors
            .iter()
            .any(|e| e.code == ValidationErrorCode::ForbiddenContent)
    );
}

#[test]
fn test_tool_params_still_warn_on_repetition() {
    let validator = Validator::new();
    let result = validator.validate_tool_params(&serde_json::json!({
        "content": format!("prefix{}suffix", "x".repeat(50))
    }));

    assert!(result.is_valid);
    assert!(
        result.warnings.iter().any(|w| w.contains("repetition")),
        "expected repetition warning for tool params, got: {:?}",
        result.warnings
    );
}

#[test]
fn test_tool_params_still_warn_on_whitespace_ratio() {
    let validator = Validator::new();
    // >100 chars, >90% whitespace
    let result = validator.validate_tool_params(&serde_json::json!({
        "content": format!("a{}b", " ".repeat(200))
    }));

    assert!(result.is_valid);
    assert!(
        result.warnings.iter().any(|w| w.contains("whitespace")),
        "expected whitespace warning for tool params, got: {:?}",
        result.warnings
    );
}

#[test]
fn test_tool_params_error_field_contains_json_path() {
    let validator = Validator::new().forbid_pattern("evil");
    let result = validator.validate_tool_params(&serde_json::json!({
        "metadata": {
            "tags": ["good", "evil"]
        }
    }));

    assert!(!result.is_valid);
    let error = result
        .errors
        .iter()
        .find(|e| e.code == ValidationErrorCode::ForbiddenContent)
        .expect("expected forbidden content error");
    assert_eq!(error.field, "metadata.tags[1]");
}
