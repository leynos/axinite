//! Unit tests for credential-grant parsing in sandbox history rows.

use super::*;

#[test]
fn test_null_description_falls_back_to_empty_array() {
    assert_eq!(credential_grants_from_description(None), "[]");
}

#[test]
fn test_valid_json_array_passes_through() {
    let json = r#"[{"secret_name":"API_KEY","env_var":"API_KEY"}]"#;
    assert_eq!(
        credential_grants_from_description(Some(json.to_string())),
        json
    );
}

#[test]
fn test_malformed_legacy_plaintext_passes_through() {
    // Legacy rows with plaintext descriptions from before the column was
    // repurposed should pass through unchanged. Normalization happens at
    // restart time via `normalize_credential_grants_json`.
    assert_eq!(
        credential_grants_from_description(Some("Build a web server".to_string())),
        "Build a web server"
    );
}

#[test]
fn test_empty_string_passes_through() {
    // The libSQL backend returns "" for NULL columns. This should pass
    // through at this layer; normalization occurs at restart.
    assert_eq!(credential_grants_from_description(Some(String::new())), "");
}
