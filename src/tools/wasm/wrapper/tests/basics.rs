//! Tests for wrapper creation, default capabilities, and URL host
//! extraction.

use super::super::*;
use crate::tools::wasm::capabilities::Capabilities;
use crate::tools::wasm::runtime::{WasmRuntimeConfig, WasmToolRuntime};

#[test]
fn test_wrapper_creation() {
    // This test verifies the runtime can be created
    // Actual execution tests require a valid WASM component
    let config = WasmRuntimeConfig::for_testing();
    let runtime = Arc::new(WasmToolRuntime::new(config).unwrap());

    // Runtime was created successfully
    assert!(runtime.config().fuel_config.enabled);
}

#[test]
fn test_capabilities_default() {
    let caps = Capabilities::default();
    assert!(caps.workspace_read.is_none());
    assert!(caps.http.is_none());
    assert!(caps.tool_invoke.is_none());
    assert!(caps.secrets.is_none());
}

#[test]
fn test_extract_host_from_url() {
    use crate::tools::wasm::wrapper::extract_host_from_url;

    assert_eq!(
        extract_host_from_url("https://www.googleapis.com/calendar/v3/events"),
        Some("www.googleapis.com".to_string())
    );
    assert_eq!(
        extract_host_from_url("https://api.example.com:443/v1/foo"),
        Some("api.example.com".to_string())
    );
    assert_eq!(
        extract_host_from_url("http://localhost:8080/test?q=1"),
        Some("localhost".to_string())
    );
    assert_eq!(
        extract_host_from_url("https://user:pass@host.com/path"),
        Some("host.com".to_string())
    );
    assert_eq!(extract_host_from_url("ftp://bad.com"), None);
    assert_eq!(extract_host_from_url("not a url"), None);
    // IPv6
    assert_eq!(
        extract_host_from_url("http://[::1]:8080/test"),
        Some("::1".to_string())
    );
    assert_eq!(
        extract_host_from_url("https://[2001:db8::1]/path"),
        Some("2001:db8::1".to_string())
    );
}
