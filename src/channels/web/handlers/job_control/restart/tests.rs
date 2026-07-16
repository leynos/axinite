//! Unit tests for normalizing credential grant JSON on job restart.

use super::*;

#[test]
fn test_normalize_credential_grants_json_valid_array() {
    let valid_array = r#"[{"tool": "test", "credential": "secret"}]"#;
    assert_eq!(normalize_credential_grants_json(valid_array), valid_array);
}

#[test]
fn test_normalize_credential_grants_json_empty_array() {
    assert_eq!(normalize_credential_grants_json("[]"), "[]");
}

#[test]
fn test_normalize_credential_grants_json_malformed_plaintext() {
    // Legacy sandbox rows may have malformed/plaintext credential_grants_json
    // This should normalize to empty array
    assert_eq!(normalize_credential_grants_json("not valid json"), "[]");
}

#[test]
fn test_normalize_credential_grants_json_non_array_object() {
    // JSON object (not array) should normalize to empty array
    assert_eq!(
        normalize_credential_grants_json(r#"{"tool": "test"}"#),
        "[]"
    );
}

#[test]
fn test_normalize_credential_grants_json_non_array_string() {
    // JSON string (not array) should normalize to empty array
    assert_eq!(
        normalize_credential_grants_json("\"plaintext string\""),
        "[]"
    );
}

#[test]
fn test_normalize_credential_grants_json_non_array_number() {
    // JSON number (not array) should normalize to empty array
    assert_eq!(normalize_credential_grants_json("42"), "[]");
}

#[test]
fn test_normalize_credential_grants_json_non_array_null() {
    // JSON null (not array) should normalize to empty array
    assert_eq!(normalize_credential_grants_json("null"), "[]");
}
