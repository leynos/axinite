//! Tests for kind inference from URLs and install fallback decision logic.

use crate::extensions::manager::{
    FallbackDecision, combine_install_errors, fallback_decision, infer_kind_from_url,
};
use crate::extensions::{ExtensionError, ExtensionKind, ExtensionSource, InstallResult};

#[test]
fn test_infer_kind_from_url() {
    assert_eq!(
        infer_kind_from_url("https://example.com/tool.wasm"),
        ExtensionKind::WasmTool
    );
    assert_eq!(
        infer_kind_from_url("https://example.com/tool-wasm32-wasip2.tar.gz"),
        ExtensionKind::WasmTool
    );
    assert_eq!(
        infer_kind_from_url("https://mcp.notion.com"),
        ExtensionKind::McpServer
    );
    assert_eq!(
        infer_kind_from_url("https://example.com/mcp"),
        ExtensionKind::McpServer
    );
}

// ---- fallback install logic tests ----

fn make_ok_result() -> Result<InstallResult, ExtensionError> {
    Ok(InstallResult {
        name: "test".to_string(),
        kind: ExtensionKind::WasmTool,
        message: "Installed".to_string(),
    })
}

fn make_fallback_source() -> Option<Box<ExtensionSource>> {
    Some(Box::new(ExtensionSource::WasmBuildable {
        source_dir: "tools-src/test".to_string(),
        build_dir: Some("tools-src/test".to_string()),
        crate_name: Some("test-tool".to_string()),
    }))
}

#[test]
fn test_fallback_decision_success_returns_directly() {
    let result = make_ok_result();
    let fallback = make_fallback_source();
    assert!(matches!(
        fallback_decision(&result, &fallback),
        FallbackDecision::Return
    ));
}

#[test]
fn test_fallback_decision_already_installed_skips_fallback() {
    let result: Result<InstallResult, ExtensionError> =
        Err(ExtensionError::AlreadyInstalled("test".to_string()));
    let fallback = make_fallback_source();
    assert!(matches!(
        fallback_decision(&result, &fallback),
        FallbackDecision::Return
    ));
}

#[test]
fn test_fallback_decision_download_failed_triggers_fallback() {
    let result: Result<InstallResult, ExtensionError> =
        Err(ExtensionError::DownloadFailed("404 Not Found".to_string()));
    let fallback = make_fallback_source();
    assert!(matches!(
        fallback_decision(&result, &fallback),
        FallbackDecision::TryFallback
    ));
}

#[test]
fn test_fallback_decision_error_without_fallback_returns() {
    let result: Result<InstallResult, ExtensionError> =
        Err(ExtensionError::DownloadFailed("404 Not Found".to_string()));
    let fallback = None;
    assert!(matches!(
        fallback_decision(&result, &fallback),
        FallbackDecision::Return
    ));
}

#[test]
fn test_combine_errors_includes_both_messages() {
    let primary = ExtensionError::DownloadFailed("404 Not Found".to_string());
    let fallback = ExtensionError::InstallFailed("cargo not found".to_string());
    let combined = combine_install_errors(primary, fallback);
    assert!(
        matches!(combined, ExtensionError::FallbackFailed { .. }),
        "Expected FallbackFailed, got: {combined:?}"
    );
    let msg = combined.to_string();
    assert!(msg.contains("404 Not Found"), "missing primary: {msg}");
    assert!(msg.contains("cargo not found"), "missing fallback: {msg}");
}

#[test]
fn test_combine_errors_forwards_already_installed_from_fallback() {
    let primary = ExtensionError::DownloadFailed("404".to_string());
    let fallback = ExtensionError::AlreadyInstalled("test".to_string());
    let combined = combine_install_errors(primary, fallback);
    assert!(
        matches!(combined, ExtensionError::AlreadyInstalled(ref name) if name == "test"),
        "Expected AlreadyInstalled, got: {combined:?}"
    );
}
