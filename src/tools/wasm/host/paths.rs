//! Workspace path validation for the WASM host.
//!
//! Blocks path traversal, absolute paths, null bytes, and Windows-style
//! drive paths before any workspace read is attempted.

use crate::tools::wasm::error::WasmError;

/// Validate a workspace path for security.
///
/// Blocks path traversal attacks and absolute paths.
pub(super) fn validate_workspace_path(path: &str) -> Result<(), WasmError> {
    // Block absolute paths
    if path.starts_with('/') {
        return Err(WasmError::PathTraversalBlocked(
            "absolute paths not allowed".to_string(),
        ));
    }

    // Block path traversal
    if path.contains("..") {
        return Err(WasmError::PathTraversalBlocked(
            "parent directory references not allowed".to_string(),
        ));
    }

    // Block null bytes
    if path.contains('\0') {
        return Err(WasmError::PathTraversalBlocked(
            "null bytes not allowed".to_string(),
        ));
    }

    // Block Windows-style absolute paths (just in case)
    if path.len() >= 2 && path.chars().nth(1) == Some(':') {
        return Err(WasmError::PathTraversalBlocked(
            "Windows-style paths not allowed".to_string(),
        ));
    }

    Ok(())
}
