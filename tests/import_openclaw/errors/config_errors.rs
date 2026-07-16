//! Missing-directory and config-file error tests for the OpenClaw importer.

use std::path::PathBuf;
use tempfile::TempDir;

use ironclaw::import::ImportError;
use ironclaw::import::openclaw::reader::OpenClawReader;

// ────────────────────────────────────────────────────────────────────
// Missing Directory Tests
// ────────────────────────────────────────────────────────────────────

#[test]
fn test_error_nonexistent_openclaw_directory() {
    let nonexistent = PathBuf::from("/nonexistent/path/openclaw");
    let result = OpenClawReader::new(&nonexistent);

    assert!(result.is_err());
    if let Err(e) = result {
        match e {
            ImportError::NotFound { .. } => (), // Expected
            _ => panic!("Expected NotFound, got: {}", e),
        }
    }
}

#[test]
fn test_error_empty_openclaw_directory() {
    let temp_dir = TempDir::new().expect("temp dir creation failed");
    let result = OpenClawReader::new(temp_dir.path());

    // Should succeed (directory exists)
    assert!(result.is_ok());

    let reader = result.unwrap();
    let config_result = reader.read_config();

    // But reading config should fail
    assert!(config_result.is_err());
}

// ────────────────────────────────────────────────────────────────────
// Config File Errors
// ────────────────────────────────────────────────────────────────────

#[test]
fn test_error_missing_openclaw_json() {
    let temp_dir = TempDir::new().expect("temp dir creation failed");
    let openclaw_path = temp_dir.path().to_path_buf();

    let reader = OpenClawReader::new(&openclaw_path).expect("reader creation failed");

    let result = reader.read_config();
    assert!(result.is_err());
}

#[test]
fn test_error_invalid_json5_syntax() {
    let temp_dir = TempDir::new().expect("temp dir creation failed");
    let openclaw_path = temp_dir.path().to_path_buf();

    // Invalid JSON5: missing closing brace
    let bad_config = r#"{ llm: { provider: "openai" }"#;
    std::fs::write(openclaw_path.join("openclaw.json"), bad_config).expect("write failed");

    let reader = OpenClawReader::new(&openclaw_path).expect("reader creation failed");

    let result = reader.read_config();
    assert!(result.is_err());
}

#[test]
fn test_error_truncated_json5() {
    let temp_dir = TempDir::new().expect("temp dir creation failed");
    let openclaw_path = temp_dir.path().to_path_buf();

    // Truncated JSON5
    std::fs::write(openclaw_path.join("openclaw.json"), "{").expect("write failed");

    let reader = OpenClawReader::new(&openclaw_path).expect("reader creation failed");

    let result = reader.read_config();
    assert!(result.is_err());
}

#[test]
fn test_error_empty_openclaw_json() {
    let temp_dir = TempDir::new().expect("temp dir creation failed");
    let openclaw_path = temp_dir.path().to_path_buf();

    // Empty file
    std::fs::write(openclaw_path.join("openclaw.json"), "").expect("write failed");

    let reader = OpenClawReader::new(&openclaw_path).expect("reader creation failed");

    let result = reader.read_config();
    assert!(result.is_err());
}
