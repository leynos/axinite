//! Unit tests for WASM tool artefact validation.

use super::*;

#[test]
fn test_validator_default() {
    let validator = WasmValidator::new();
    assert_eq!(validator.max_size, 10 * 1024 * 1024);
    assert!(validator.required_exports.contains(&"run".to_string()));
}

#[test]
fn test_validator_builder() {
    let validator = WasmValidator::new()
        .with_max_size(1024)
        .with_required_export("custom_export")
        .with_allowed_import("custom_module");

    assert_eq!(validator.max_size, 1024);
    assert!(
        validator
            .required_exports
            .contains(&"custom_export".to_string())
    );
    assert!(
        validator
            .allowed_import_modules
            .contains(&"custom_module".to_string())
    );
}

#[test]
fn test_validate_bytes_invalid_bytes() {
    let validator = WasmValidator::new();
    let garbage = b"this is not a wasm module at all";
    let result = validator.validate_bytes(garbage).unwrap();
    assert!(!result.is_valid);
    assert!(
        result
            .errors
            .iter()
            .any(|e| matches!(e, ValidationError::InvalidModule(_)))
    );
}

#[test]
fn test_validate_bytes_empty() {
    let validator = WasmValidator::new();
    let result = validator.validate_bytes(b"").unwrap();
    assert!(!result.is_valid);
    assert!(
        result
            .errors
            .iter()
            .any(|e| matches!(e, ValidationError::InvalidModule(_)))
    );
}

#[test]
fn test_validate_bytes_minimal_wasm_missing_run_export() {
    let validator = WasmValidator::new();
    // Minimal valid WASM: magic number + version
    let minimal_wasm = b"\x00asm\x01\x00\x00\x00";
    let result = validator.validate_bytes(minimal_wasm).unwrap();
    assert!(!result.is_valid);
    assert!(
        result
            .errors
            .iter()
            .any(|e| matches!(e, ValidationError::MissingExport(name) if name == "run"))
    );
    assert_eq!(result.size_bytes, 8);
}

#[test]
fn test_validation_result_is_valid_when_no_errors() {
    let result = ValidationResult {
        is_valid: true,
        errors: vec![],
        warnings: vec!["some warning".to_string()],
        exports: vec![],
        imports: vec![],
        size_bytes: 0,
    };
    assert!(result.is_valid);
    assert!(result.errors.is_empty());
}

#[test]
fn test_validation_result_is_invalid_when_errors_present() {
    let result = ValidationResult {
        is_valid: false,
        errors: vec![ValidationError::MissingExport("run".to_string())],
        warnings: vec![],
        exports: vec![],
        imports: vec![],
        size_bytes: 0,
    };
    assert!(!result.is_valid);
    assert_eq!(result.errors.len(), 1);
}

#[test]
fn test_validation_error_display() {
    let io_err =
        ValidationError::IoError(std::io::Error::new(std::io::ErrorKind::NotFound, "gone"));
    assert!(io_err.to_string().contains("Failed to read WASM file"));

    let invalid = ValidationError::InvalidModule("bad magic".to_string());
    assert!(invalid.to_string().contains("Invalid WASM module"));
    assert!(invalid.to_string().contains("bad magic"));

    let missing = ValidationError::MissingExport("run".to_string());
    assert!(missing.to_string().contains("Missing required export"));
    assert!(missing.to_string().contains("run"));

    let sig = ValidationError::InvalidSignature {
        name: "run".to_string(),
        expected: "() -> i32".to_string(),
        actual: "() -> ()".to_string(),
    };
    assert!(sig.to_string().contains("Invalid export signature"));
    assert!(sig.to_string().contains("run"));

    let disallowed = ValidationError::DisallowedImport {
        module: "evil".to_string(),
        name: "hack".to_string(),
    };
    assert!(disallowed.to_string().contains("disallowed import"));
    assert!(disallowed.to_string().contains("evil::hack"));

    let too_large = ValidationError::TooLarge {
        size: 200,
        max: 100,
    };
    assert!(too_large.to_string().contains("200"));
    assert!(too_large.to_string().contains("100"));

    let other = ValidationError::Other("something broke".to_string());
    assert!(other.to_string().contains("something broke"));
}

#[test]
fn test_export_kind_equality() {
    assert_eq!(ExportKind::Function, ExportKind::Function);
    assert_eq!(ExportKind::Memory, ExportKind::Memory);
    assert_eq!(ExportKind::Table, ExportKind::Table);
    assert_eq!(ExportKind::Global, ExportKind::Global);
    assert_ne!(ExportKind::Function, ExportKind::Memory);
    assert_ne!(ExportKind::Table, ExportKind::Global);
}

#[test]
fn test_import_kind_equality() {
    assert_eq!(ImportKind::Function, ImportKind::Function);
    assert_eq!(ImportKind::Memory, ImportKind::Memory);
    assert_eq!(ImportKind::Table, ImportKind::Table);
    assert_eq!(ImportKind::Global, ImportKind::Global);
    assert_ne!(ImportKind::Function, ImportKind::Global);
    assert_ne!(ImportKind::Memory, ImportKind::Table);
}

#[test]
fn test_validate_bytes_exceeds_max_size() {
    let validator = WasmValidator::new().with_max_size(4);
    // 8 bytes, over the 4-byte limit
    let minimal_wasm = b"\x00asm\x01\x00\x00\x00";
    let result = validator.validate_bytes(minimal_wasm).unwrap();
    assert!(!result.is_valid);
    assert!(
        result
            .errors
            .iter()
            .any(|e| matches!(e, ValidationError::TooLarge { size: 8, max: 4 }))
    );
}

#[test]
fn test_with_max_size_then_validate_over_limit() {
    let validator = WasmValidator::new().with_max_size(16);
    let oversized = vec![0u8; 32];
    let result = validator.validate_bytes(&oversized).unwrap();
    assert!(!result.is_valid);
    assert!(
        result
            .errors
            .iter()
            .any(|e| matches!(e, ValidationError::TooLarge { size: 32, max: 16 }))
    );
}
