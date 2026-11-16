//! WASM tool validation.
//!
//! Validates that built WASM modules conform to the expected tool interface
//! before they can be registered with the agent.

use std::path::Path;

use thiserror::Error;

/// Errors during WASM validation.
#[derive(Debug, Error)]
pub enum ValidationError {
    /// The WASM file could not be read from disk.
    #[error("Failed to read WASM file: {0}")]
    IoError(#[from] std::io::Error),

    /// The bytes do not parse as a WASM module, or an export or import entry is malformed.
    #[error("Invalid WASM module: {0}")]
    InvalidModule(String),

    /// A required export, such as `run`, is absent. The string is its name.
    #[error("Missing required export: {0}")]
    MissingExport(String),

    /// An export exists but its function signature differs from the one required.
    #[error("Invalid export signature for '{name}': expected {expected}, got {actual}")]
    InvalidSignature {
        /// Export whose signature was checked.
        name: String,
        /// Signature the interface requires.
        expected: String,
        /// Signature the module declares.
        actual: String,
    },

    /// The module imports from a namespace that is not on the allowed list.
    #[error("Module uses disallowed import: {module}::{name}")]
    DisallowedImport {
        /// Import namespace, such as `env`.
        module: String,
        /// Name of the imported item.
        name: String,
    },

    /// The module is larger than the validator's size limit.
    #[error("Module exceeds size limit: {size} bytes (max: {max} bytes)")]
    TooLarge {
        /// Module size in bytes.
        size: u64,
        /// Size limit in bytes.
        max: u64,
    },

    /// Any other validation failure. The string is the message.
    #[error("Validation failed: {0}")]
    Other(String),
}

/// Result of WASM validation.
#[derive(Debug)]
pub struct ValidationResult {
    /// Whether the module is valid.
    pub is_valid: bool,
    /// List of validation errors (empty if valid).
    pub errors: Vec<ValidationError>,
    /// List of warnings (non-fatal issues).
    pub warnings: Vec<String>,
    /// Detected exports.
    pub exports: Vec<ExportInfo>,
    /// Detected imports.
    pub imports: Vec<ImportInfo>,
    /// Module size in bytes.
    pub size_bytes: u64,
}

/// Information about an exported function.
#[derive(Debug, Clone)]
pub struct ExportInfo {
    pub name: String,
    pub kind: ExportKind,
}

/// Kind of export.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExportKind {
    Function,
    Memory,
    Table,
    Global,
}

/// Information about an imported function.
#[derive(Debug, Clone)]
pub struct ImportInfo {
    pub module: String,
    pub name: String,
    pub kind: ImportKind,
}

/// Kind of import.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImportKind {
    Function,
    Memory,
    Table,
    Global,
}

/// Validator for WASM tool modules.
pub struct WasmValidator {
    /// Maximum module size in bytes.
    max_size: u64,
    /// Required exports that must be present.
    required_exports: Vec<String>,
    /// Allowed import modules.
    allowed_import_modules: Vec<String>,
}

impl Default for WasmValidator {
    fn default() -> Self {
        Self {
            max_size: 10 * 1024 * 1024, // 10 MB
            required_exports: vec!["run".to_string()],
            allowed_import_modules: vec![
                "env".to_string(),
                "wasi_snapshot_preview1".to_string(),
                "wasi".to_string(),
            ],
        }
    }
}

impl WasmValidator {
    /// Create a validator with the default 10 MB limit, requiring a `run` export and
    /// allowing only the `env` and WASI import namespaces.
    pub fn new() -> Self {
        Self::default()
    }

    /// Set maximum module size.
    pub fn with_max_size(mut self, max_bytes: u64) -> Self {
        self.max_size = max_bytes;
        self
    }

    /// Add a required export.
    pub fn with_required_export(mut self, name: impl Into<String>) -> Self {
        self.required_exports.push(name.into());
        self
    }

    /// Add an allowed import module.
    pub fn with_allowed_import(mut self, module: impl Into<String>) -> Self {
        self.allowed_import_modules.push(module.into());
        self
    }

    /// Validate a WASM file.
    pub async fn validate_file(&self, path: &Path) -> Result<ValidationResult, ValidationError> {
        let bytes = tokio::fs::read(path).await?;
        self.validate_bytes(&bytes)
    }

    /// Validate WASM bytes.
    pub fn validate_bytes(&self, bytes: &[u8]) -> Result<ValidationResult, ValidationError> {
        let mut parsed = ParsedSections::default();
        let mut warnings = Vec::new();
        let size_bytes = bytes.len() as u64;

        // Check size
        if size_bytes > self.max_size {
            parsed.errors.push(ValidationError::TooLarge {
                size: size_bytes,
                max: self.max_size,
            });
        }

        collect_sections(bytes, &self.allowed_import_modules, &mut parsed);

        push_missing_exports(&self.required_exports, &parsed.exports, &mut parsed.errors);

        collect_warnings(&parsed.exports, &parsed.imports, &mut warnings);

        Ok(ValidationResult {
            is_valid: parsed.errors.is_empty(),
            errors: parsed.errors,
            warnings,
            exports: parsed.exports,
            imports: parsed.imports,
            size_bytes,
        })
    }
}

/// Exports, imports and parse errors gathered from a module's sections.
#[derive(Default)]
struct ParsedSections {
    exports: Vec<ExportInfo>,
    imports: Vec<ImportInfo>,
    errors: Vec<ValidationError>,
}

/// Walk the parsed WASM sections, recording exports, imports, and parse errors.
fn collect_sections(bytes: &[u8], allowed_import_modules: &[String], parsed: &mut ParsedSections) {
    let parser = wasmparser::Parser::new(0);

    for payload in parser.parse_all(bytes) {
        match payload {
            Ok(wasmparser::Payload::ExportSection(reader)) => {
                collect_export_section(reader, &mut parsed.errors, &mut parsed.exports);
            }
            Ok(wasmparser::Payload::ImportSection(reader)) => {
                collect_import_section(
                    reader,
                    allowed_import_modules,
                    &mut parsed.errors,
                    &mut parsed.imports,
                );
            }
            Ok(_) => {
                // Other sections are OK
            }
            Err(e) => {
                parsed.errors.push(ValidationError::InvalidModule(format!(
                    "Failed to parse WASM: {}",
                    e
                )));
                break;
            }
        }
    }
}

/// Record every export in one export section, skipping tag exports.
fn collect_export_section(
    reader: wasmparser::ExportSectionReader<'_>,
    errors: &mut Vec<ValidationError>,
    exports: &mut Vec<ExportInfo>,
) {
    for export in reader {
        match export {
            Ok(exp) => {
                if let Some(kind) = export_kind(exp.kind) {
                    exports.push(ExportInfo {
                        name: exp.name.to_string(),
                        kind,
                    });
                }
            }
            Err(e) => {
                errors.push(ValidationError::InvalidModule(format!(
                    "Failed to parse export: {}",
                    e
                )));
            }
        }
    }
}

/// Map a WASM export kind, returning `None` for tags the validator ignores.
fn export_kind(kind: wasmparser::ExternalKind) -> Option<ExportKind> {
    match kind {
        wasmparser::ExternalKind::Func => Some(ExportKind::Function),
        wasmparser::ExternalKind::Memory => Some(ExportKind::Memory),
        wasmparser::ExternalKind::Table => Some(ExportKind::Table),
        wasmparser::ExternalKind::Global => Some(ExportKind::Global),
        wasmparser::ExternalKind::Tag => None,
    }
}

/// Record every import in one import section, rejecting disallowed modules and
/// skipping tag imports.
fn collect_import_section(
    reader: wasmparser::ImportSectionReader<'_>,
    allowed_import_modules: &[String],
    errors: &mut Vec<ValidationError>,
    imports: &mut Vec<ImportInfo>,
) {
    for import in reader {
        match import {
            Ok(imp) => {
                if let Some(kind) = import_kind(imp.ty) {
                    imports.push(ImportInfo {
                        module: imp.module.to_string(),
                        name: imp.name.to_string(),
                        kind,
                    });

                    // Check if import module is allowed
                    if !allowed_import_modules.contains(&imp.module.to_string()) {
                        errors.push(ValidationError::DisallowedImport {
                            module: imp.module.to_string(),
                            name: imp.name.to_string(),
                        });
                    }
                }
            }
            Err(e) => {
                errors.push(ValidationError::InvalidModule(format!(
                    "Failed to parse import: {}",
                    e
                )));
            }
        }
    }
}

/// Map a WASM import type, returning `None` for tags the validator ignores.
fn import_kind(ty: wasmparser::TypeRef) -> Option<ImportKind> {
    match ty {
        wasmparser::TypeRef::Func(_) => Some(ImportKind::Function),
        wasmparser::TypeRef::Memory(_) => Some(ImportKind::Memory),
        wasmparser::TypeRef::Table(_) => Some(ImportKind::Table),
        wasmparser::TypeRef::Global(_) => Some(ImportKind::Global),
        wasmparser::TypeRef::Tag(_) => None,
    }
}

/// Record a missing-export error for every required export not present.
fn push_missing_exports(
    required_exports: &[String],
    exports: &[ExportInfo],
    errors: &mut Vec<ValidationError>,
) {
    for required in required_exports {
        if !exports.iter().any(|e| &e.name == required) {
            errors.push(ValidationError::MissingExport(required.clone()));
        }
    }
}

/// Record warnings for a missing memory export and for dangerous WASI imports.
fn collect_warnings(exports: &[ExportInfo], imports: &[ImportInfo], warnings: &mut Vec<String>) {
    if !exports
        .iter()
        .any(|e| e.name == "memory" && e.kind == ExportKind::Memory)
    {
        warnings.push("Module does not export memory - host cannot read/write data".to_string());
    }

    for import in imports {
        push_import_warnings(import, warnings);
    }
}

/// Record a warning for each WASI filesystem or socket function an import uses.
fn push_import_warnings(import: &ImportInfo, warnings: &mut Vec<String>) {
    if import.module == "wasi_snapshot_preview1" {
        match import.name.as_str() {
            "fd_write" | "fd_read" | "path_open" | "path_create_directory" => {
                warnings.push(format!(
                    "Module uses WASI filesystem function '{}' - ensure this is intended",
                    import.name
                ));
            }
            "sock_send" | "sock_recv" | "sock_accept" => {
                warnings.push(format!(
                    "Module uses WASI socket function '{}' - ensure this is intended",
                    import.name
                ));
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests;
