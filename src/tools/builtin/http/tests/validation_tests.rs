//! Tests for URL/IP/path validation, header parsing, and the tool schema.

use std::net::IpAddr;

use crate::tools::builtin::http::HttpTool;
use crate::tools::builtin::http::MAX_RESPONSE_SIZE;
use crate::tools::builtin::http::validation::{
    extract_host_from_params, is_disallowed_ip, parse_headers_param, validate_save_to_path,
    validate_url,
};
use crate::tools::tool::NativeTool;

#[test]
fn test_http_tool_schema_headers_is_array() {
    let tool = HttpTool::new().expect("Failed to create HTTP client");
    let schema = tool.parameters_schema();
    assert_eq!(schema["properties"]["headers"]["type"], "array");
}

#[test]
fn test_validate_url_rejects_http() {
    let err = validate_url("http://example.com").unwrap_err();
    assert!(err.to_string().contains("https"));
}

#[test]
fn test_validate_url_rejects_localhost() {
    let err = validate_url("https://localhost:8080").unwrap_err();
    assert!(err.to_string().contains("localhost"));
}

#[test]
fn test_validate_url_accepts_https_public() {
    let url = validate_url("https://example.com").unwrap();
    assert_eq!(url.host_str(), Some("example.com"));
}

#[test]
fn test_validate_url_rejects_private_ip_literal() {
    let err = validate_url("https://192.168.1.1/api").unwrap_err();
    assert!(err.to_string().contains("private"));
}

#[test]
fn test_validate_url_rejects_loopback_ip() {
    let err = validate_url("https://127.0.0.1/api").unwrap_err();
    assert!(err.to_string().contains("private"));
}

#[test]
fn test_validate_url_rejects_link_local() {
    let err = validate_url("https://169.254.169.254/latest/meta-data/").unwrap_err();
    assert!(err.to_string().contains("private"));
}

#[test]
fn test_is_disallowed_ip_covers_ranges() {
    use std::net::Ipv4Addr;

    // Private ranges
    assert!(is_disallowed_ip(&IpAddr::V4(Ipv4Addr::new(10, 0, 0, 1))));
    assert!(is_disallowed_ip(&IpAddr::V4(Ipv4Addr::new(172, 16, 0, 1))));
    assert!(is_disallowed_ip(&IpAddr::V4(Ipv4Addr::new(192, 168, 0, 1))));
    // Loopback
    assert!(is_disallowed_ip(&IpAddr::V4(Ipv4Addr::LOCALHOST)));
    // Cloud metadata
    assert!(is_disallowed_ip(&IpAddr::V4(Ipv4Addr::new(
        169, 254, 169, 254
    ))));
    // Public
    assert!(!is_disallowed_ip(&IpAddr::V4(Ipv4Addr::new(8, 8, 8, 8))));
}

#[test]
fn test_max_response_size_is_reasonable() {
    // MAX_RESPONSE_SIZE should be 5 MB to prevent OOM while allowing typical API responses.
    assert_eq!(MAX_RESPONSE_SIZE, 5 * 1024 * 1024);
}

#[test]
fn test_parse_headers_param_accepts_object_legacy_shape() {
    let headers = serde_json::json!({"Authorization": "Bearer token"});
    let parsed = parse_headers_param(Some(&headers)).unwrap();
    assert_eq!(
        parsed,
        vec![("Authorization".to_string(), "Bearer token".to_string())]
    );
}

#[test]
fn test_parse_headers_param_accepts_array_shape() {
    let headers = serde_json::json!([
        {"name": "Authorization", "value": "Bearer token"},
        {"name": "X-Test", "value": "1"}
    ]);
    let parsed = parse_headers_param(Some(&headers)).unwrap();
    assert_eq!(
        parsed,
        vec![
            ("Authorization".to_string(), "Bearer token".to_string()),
            ("X-Test".to_string(), "1".to_string())
        ]
    );
}

#[test]
fn test_http_tool_schema_body_is_freeform() {
    let schema = HttpTool::new()
        .expect("Failed to create HTTP client")
        .parameters_schema();
    let body = schema
        .get("properties")
        .and_then(|p| p.get("body"))
        .expect("body schema missing");

    // Body is intentionally freeform (no "type" constraint) for OpenAI
    // compatibility. OpenAI rejects union types containing "array" unless
    // "items" is also specified, and body accepts any JSON value.
    assert!(
        body.get("type").is_none(),
        "body schema should not have a 'type' to be freeform for OpenAI compatibility"
    );
}

#[test]
fn test_extract_host_from_params_valid() {
    let params = serde_json::json!({
        "url": "https://api.example.com/path"
    });
    assert_eq!(
        extract_host_from_params(&params),
        Some("api.example.com".to_string())
    );
}

#[test]
fn test_extract_host_from_params_missing_url() {
    let params = serde_json::json!({"method": "GET"});
    assert_eq!(extract_host_from_params(&params), None);
}

// ── save_to path validation tests ─────────────────────────────────────

#[test]
fn test_save_to_rejects_path_outside_tmp() {
    let err = validate_save_to_path("/etc/passwd").unwrap_err();
    assert!(err.to_string().contains("must be under /tmp/"));
}

#[test]
fn test_save_to_rejects_home_dir() {
    let err = validate_save_to_path("/home/user/file.txt").unwrap_err();
    assert!(err.to_string().contains("must be under /tmp/"));
}

#[test]
fn test_save_to_rejects_traversal_via_dotdot() {
    let err = validate_save_to_path("/tmp/../../etc/passwd").unwrap_err();
    let msg = err.to_string();
    assert!(
        msg.contains("escapes") || msg.contains("resolves outside"),
        "expected path traversal rejection, got: {}",
        msg
    );
}

#[test]
fn test_save_to_rejects_deep_traversal() {
    let err = validate_save_to_path("/tmp/a/b/../../../../etc/shadow").unwrap_err();
    let msg = err.to_string();
    assert!(
        msg.contains("escapes") || msg.contains("resolves outside"),
        "expected path traversal rejection, got: {}",
        msg
    );
}

#[test]
fn test_save_to_accepts_simple_tmp_path() {
    let path = validate_save_to_path("/tmp/test_axinite_photo.jpg").unwrap();
    assert!(path.starts_with("/tmp"));
    let _ = ambient_fs::remove_file(&path);
}

#[test]
fn test_save_to_accepts_nested_tmp_path() {
    let path = validate_save_to_path("/tmp/axinite_test_subdir/nested/file.png").unwrap();
    assert!(path.starts_with("/tmp"));
    let _ = ambient_fs::remove_dir_all("/tmp/axinite_test_subdir");
}

#[test]
fn test_save_to_rejects_bare_tmp() {
    let err = validate_save_to_path("/tmp").unwrap_err();
    assert!(err.to_string().contains("must be under /tmp/"));
}
