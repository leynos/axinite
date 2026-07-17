//! Tests for attachment path validation and conversation-context extraction.

use super::*;

// ── attachment path validation ───────────────────────────────────

#[test]
fn validate_attachment_paths_rejects_double_dot() {
    let paths = vec!["../etc/passwd".to_string()];
    let result = SignalChannel::validate_attachment_paths(&paths);
    assert!(result.is_err());
    let err = result.unwrap_err().to_string();
    assert!(err.contains("forbidden") || err.contains("sandbox"));
}

#[test]
fn validate_attachment_paths_accepts_normal_paths() {
    use ambient_fs as fs;

    // Create test files in sandbox
    let base_dir = crate::bootstrap::axinite_base_dir();

    // Create sandbox directory if it doesn't exist (needed for CI)
    let _ = fs::create_dir_all(&base_dir);

    let temp_dir = tempfile::tempdir_in(&base_dir).unwrap();
    let file1 = temp_dir.path().join("file.txt");
    let file2 = temp_dir.path().join("report.pdf");
    fs::write(&file1, "test").unwrap();
    fs::write(&file2, "test").unwrap();

    let paths = vec![
        file1.to_string_lossy().to_string(),
        file2.to_string_lossy().to_string(),
    ];
    let result = SignalChannel::validate_attachment_paths(&paths);
    assert!(result.is_ok());
}

#[test]
fn validate_attachment_paths_rejects_nested_traversal() {
    let paths = vec!["foo/../bar/../../secret.txt".to_string()];
    let result = SignalChannel::validate_attachment_paths(&paths);
    assert!(result.is_err());
}

#[test]
fn validate_attachment_paths_empty_ok() {
    let paths: Vec<String> = vec![];
    let result = SignalChannel::validate_attachment_paths(&paths);
    assert!(result.is_ok());
}

#[test]
fn validate_attachment_paths_rejects_path_outside_sandbox() {
    let paths = vec!["/tmp/evil.txt".to_string()];
    let result = SignalChannel::validate_attachment_paths(&paths);
    assert!(result.is_err());
    let err = result.unwrap_err().to_string();
    assert!(err.contains("sandbox"));
}

#[test]
fn validate_attachment_paths_rejects_url_encoded_traversal() {
    let paths = vec!["%2e%2e%2fetc/passwd".to_string()];
    let result = SignalChannel::validate_attachment_paths(&paths);
    assert!(result.is_err());
}

#[test]
fn validate_attachment_paths_rejects_null_byte() {
    let paths = vec!["file\0.txt".to_string()];
    let result = SignalChannel::validate_attachment_paths(&paths);
    assert!(result.is_err());
}

// ── conversation context ───────────────────────────────────────────

#[test]
fn conversation_context_extracts_sender() {
    let ch = SignalChannel::new(make_config()).unwrap();
    let metadata = serde_json::json!({
        "signal_sender": "+1234567890",
        "signal_sender_uuid": "uuid-123",
        "signal_target": "+0987654321"
    });
    let ctx = ch.conversation_context(&metadata);
    assert_eq!(ctx.get("sender"), Some(&"+1234567890".to_string()));
    assert_eq!(ctx.get("sender_uuid"), Some(&"uuid-123".to_string()));
    assert!(!ctx.contains_key("group"));
}

#[test]
fn conversation_context_extracts_group() {
    let ch = SignalChannel::new(make_config()).unwrap();
    let metadata = serde_json::json!({
        "signal_sender": "+1234567890",
        "signal_target": "group:mygroup"
    });
    let ctx = ch.conversation_context(&metadata);
    assert_eq!(ctx.get("sender"), Some(&"+1234567890".to_string()));
    assert_eq!(ctx.get("group"), Some(&"group:mygroup".to_string()));
}

#[test]
fn conversation_context_empty_for_unknown_channel() {
    let ch = SignalChannel::new(make_config()).unwrap();
    let metadata = serde_json::json!({
        "unknown_key": "value"
    });
    let ctx = ch.conversation_context(&metadata);
    assert!(ctx.is_empty());
}
