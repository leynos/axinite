//! Tests for approval requirements and credential-registry handling.

use std::sync::Arc;

use crate::testing::credentials::{TEST_OPENAI_API_KEY, test_secrets_store};
use crate::tools::builtin::http::HttpTool;
use crate::tools::tool::{ApprovalRequirement, NativeTool};

// ── Approval requirement tests ──────────────────────────────────────

#[test]
fn test_no_auth_headers_returns_unless_auto_approved() {
    let tool = HttpTool::new().expect("Failed to create HTTP client");
    let params = serde_json::json!({
        "method": "GET",
        "url": "https://api.example.com/data"
    });
    assert_eq!(
        tool.requires_approval(&params),
        ApprovalRequirement::UnlessAutoApproved
    );
}

#[test]
fn test_auth_header_object_format_returns_always() {
    let tool = HttpTool::new().expect("Failed to create HTTP client");
    let params = serde_json::json!({
        "method": "GET",
        "url": "https://api.example.com/data",
        "headers": {"Authorization": "Bearer token123"}
    });
    assert_eq!(tool.requires_approval(&params), ApprovalRequirement::Always);
}

#[test]
fn test_auth_header_array_format_returns_always() {
    let tool = HttpTool::new().expect("Failed to create HTTP client");
    let params = serde_json::json!({
        "method": "GET",
        "url": "https://api.example.com/data",
        "headers": [{"name": "Authorization", "value": "Bearer token123"}]
    });
    assert_eq!(tool.requires_approval(&params), ApprovalRequirement::Always);
}

#[test]
fn test_auth_header_case_insensitive() {
    let tool = HttpTool::new().expect("Failed to create HTTP client");

    // Object format with mixed case
    let params = serde_json::json!({
        "method": "GET",
        "url": "https://example.com",
        "headers": {"AUTHORIZATION": "Bearer x"}
    });
    assert_eq!(tool.requires_approval(&params), ApprovalRequirement::Always);

    // Array format with mixed case
    let params = serde_json::json!({
        "method": "GET",
        "url": "https://example.com",
        "headers": [{"name": "X-Api-Key", "value": "key123"}]
    });
    assert_eq!(tool.requires_approval(&params), ApprovalRequirement::Always);
}

#[test]
fn test_all_auth_header_names_detected() {
    let tool = HttpTool::new().expect("Failed to create HTTP client");
    for header_name in [
        "authorization",
        "x-api-key",
        "cookie",
        "proxy-authorization",
        "x-auth-token",
        "api-key",
        "x-token",
        "x-access-token",
        "x-session-token",
        "x-csrf-token",
        "x-secret",
        "x-api-secret",
    ] {
        let mut headers = serde_json::Map::new();
        headers.insert(header_name.to_string(), serde_json::json!("value"));
        let params = serde_json::json!({
            "method": "GET",
            "url": "https://example.com",
            "headers": headers
        });
        assert_eq!(
            tool.requires_approval(&params),
            ApprovalRequirement::Always,
            "Header '{}' should trigger Always approval",
            header_name
        );
    }
}

#[test]
fn test_non_auth_headers_return_unless_auto_approved() {
    let tool = HttpTool::new().expect("Failed to create HTTP client");
    let params = serde_json::json!({
        "method": "GET",
        "url": "https://example.com",
        "headers": {"Content-Type": "application/json", "Accept": "text/html"}
    });
    assert_eq!(
        tool.requires_approval(&params),
        ApprovalRequirement::UnlessAutoApproved
    );
}

#[test]
fn test_empty_headers_return_unless_auto_approved() {
    let tool = HttpTool::new().expect("Failed to create HTTP client");

    // Empty object
    let params = serde_json::json!({
        "method": "GET",
        "url": "https://example.com",
        "headers": {}
    });
    assert_eq!(
        tool.requires_approval(&params),
        ApprovalRequirement::UnlessAutoApproved
    );

    // Empty array
    let params = serde_json::json!({
        "method": "GET",
        "url": "https://example.com",
        "headers": []
    });
    assert_eq!(
        tool.requires_approval(&params),
        ApprovalRequirement::UnlessAutoApproved
    );
}

// ── Credential registry approval tests ─────────────────────────────

#[test]
fn test_host_with_credential_mapping_returns_always() {
    use crate::secrets::CredentialMapping;
    use crate::tools::wasm::SharedCredentialRegistry;

    let registry = Arc::new(SharedCredentialRegistry::new());
    registry.add_mappings(vec![CredentialMapping::bearer(
        "openai_key",
        "api.openai.com",
    )]);

    let tool = HttpTool::new()
        .expect("Failed to create HTTP client")
        .with_credentials(
            registry,
            // secrets_store is not used in requires_approval, just needs to be present
            Arc::new(test_secrets_store()),
        );

    let params = serde_json::json!({
        "method": "GET",
        "url": "https://api.openai.com/v1/models"
    });
    assert_eq!(tool.requires_approval(&params), ApprovalRequirement::Always);
}

#[test]
fn test_host_without_credential_mapping_returns_unless_auto_approved() {
    use crate::tools::wasm::SharedCredentialRegistry;

    let registry = Arc::new(SharedCredentialRegistry::new());
    // Empty registry - no credential mappings

    let tool = HttpTool::new()
        .expect("Failed to create HTTP client")
        .with_credentials(registry, Arc::new(test_secrets_store()));

    let params = serde_json::json!({
        "method": "GET",
        "url": "https://api.example.com/data"
    });
    assert_eq!(
        tool.requires_approval(&params),
        ApprovalRequirement::UnlessAutoApproved
    );
}

#[test]
fn test_url_query_param_credential_returns_always() {
    let tool = HttpTool::new().expect("Failed to create HTTP client");
    let params = serde_json::json!({
        "method": "GET",
        "url": "https://api.example.com/data?api_key=secret123"
    });
    assert_eq!(tool.requires_approval(&params), ApprovalRequirement::Always);
}

#[test]
fn test_bearer_value_in_custom_header_returns_always() {
    let tool = HttpTool::new().expect("Failed to create HTTP client");
    let params = serde_json::json!({
        "method": "GET",
        "url": "https://example.com",
        "headers": {"X-Custom": format!("Bearer {TEST_OPENAI_API_KEY}")}
    });
    assert_eq!(tool.requires_approval(&params), ApprovalRequirement::Always);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn requires_approval_multi_thread_no_panic() {
    use crate::secrets::CredentialMapping;
    use crate::tools::wasm::SharedCredentialRegistry;

    // Test with credential registry (uses std::sync::RwLock - should be safe)
    let registry = Arc::new(SharedCredentialRegistry::new());
    registry.add_mappings(vec![CredentialMapping::bearer("test_key", "api.test.com")]);

    let tool = HttpTool::new()
        .expect("Failed to create HTTP client")
        .with_credentials(registry, Arc::new(test_secrets_store()));

    // These calls should not panic in multi-thread runtime
    let params_no_auth = serde_json::json!({
        "method": "GET",
        "url": "https://api.example.com/data"
    });
    let _ = tool.requires_approval(&params_no_auth);

    let params_with_cred = serde_json::json!({
        "method": "GET",
        "url": "https://api.test.com/v1/models"
    });
    let _ = tool.requires_approval(&params_with_cred);

    let params_with_auth = serde_json::json!({
        "method": "GET",
        "url": "https://api.example.com",
        "headers": {"Authorization": "Bearer token"}
    });
    let _ = tool.requires_approval(&params_with_auth);
}
