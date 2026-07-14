//! Unit tests for MCP request construction and client behaviour.

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::Ordering;

use crate::tools::mcp::config::McpServerConfig;
use crate::tools::mcp::http_transport::HttpMcpTransport;
use crate::tools::mcp::protocol::{McpRequest, McpResponse};
use crate::tools::mcp::transport::{McpTransport, NativeMcpTransport};
use crate::tools::tool::ToolError;

use super::core::{McpClient, extract_server_name};
use super::wrapper::strip_top_level_nulls;

#[test]
fn test_mcp_request_list_tools() {
    let req = McpRequest::list_tools(1);
    assert_eq!(req.method, "tools/list");
    assert_eq!(req.id, Some(1));
}

#[test]
fn test_mcp_request_call_tool() {
    let req = McpRequest::call_tool(2, "test", serde_json::json!({"key": "value"}));
    assert_eq!(req.method, "tools/call");
    assert!(req.params.is_some());
}

#[test]
fn test_extract_server_name() {
    assert_eq!(
        extract_server_name("https://mcp.notion.com/v1"),
        "mcp_notion_com"
    );
    assert_eq!(extract_server_name("http://localhost:8080"), "localhost");
    assert_eq!(extract_server_name("invalid"), "unknown");
}

#[test]
fn test_simple_client_creation() {
    let client = McpClient::new("http://localhost:8080");
    assert_eq!(client.server_url(), "http://localhost:8080");
    assert!(client.session_manager.is_none());
    assert!(client.secrets.is_none());
}

#[test]
fn test_extract_server_name_with_port() {
    assert_eq!(
        extract_server_name("http://example.com:3000"),
        "example_com"
    );
}

#[test]
fn test_extract_server_name_with_path() {
    assert_eq!(
        extract_server_name("http://api.server.io/v2/mcp"),
        "api_server_io"
    );
}

#[test]
fn test_extract_server_name_with_query_params() {
    assert_eq!(
        extract_server_name("http://mcp.example.com/endpoint?token=abc&v=1"),
        "mcp_example_com"
    );
}

#[test]
fn test_extract_server_name_https() {
    assert_eq!(
        extract_server_name("https://secure.mcp.dev"),
        "secure_mcp_dev"
    );
}

#[test]
fn test_extract_server_name_ip_address() {
    assert_eq!(
        extract_server_name("http://192.168.1.100:9090/mcp"),
        "192_168_1_100"
    );
}

#[test]
fn test_new_defaults() {
    let client = McpClient::new("http://localhost:9999");
    assert_eq!(client.server_url(), "http://localhost:9999");
    assert_eq!(client.server_name(), "localhost");
    assert!(client.session_manager.is_none());
    assert!(client.secrets.is_none());
    assert_eq!(client.user_id, "default");
}

#[test]
fn test_new_with_name_uses_custom_name() {
    let client = McpClient::new_with_name("my-server", "http://localhost:8080");
    assert_eq!(client.server_name(), "my-server");
    assert_eq!(client.server_url(), "http://localhost:8080");
    assert_eq!(client.user_id, "default");
    assert!(client.session_manager.is_none());
    assert!(client.secrets.is_none());
}

#[test]
fn test_server_name_accessor() {
    let client = McpClient::new("https://tools.example.org/mcp");
    assert_eq!(client.server_name(), "tools_example_org");
}

#[test]
fn test_server_url_accessor() {
    let url = "https://tools.example.org/mcp?v=2";
    let client = McpClient::new(url);
    assert_eq!(client.server_url(), url);
}

#[test]
fn test_clone_preserves_fields() {
    let client = McpClient::new_with_name("cloned-server", "http://localhost:5555");
    client.next_request_id();
    client.next_request_id();
    let cloned = client.clone();
    assert_eq!(cloned.server_url(), "http://localhost:5555");
    assert_eq!(cloned.server_name(), "cloned-server");
    assert_eq!(cloned.user_id, "default");
    assert_eq!(cloned.next_id.load(Ordering::SeqCst), 3);
}

#[tokio::test]
async fn test_clone_resets_tools_cache() {
    let client = McpClient::new("http://localhost:5555");
    let cloned = client.clone();
    let cache = cloned.tools_cache.read().await;
    assert!(cache.is_none());
}

#[test]
fn test_new_with_config_carries_custom_headers() {
    let mut headers = HashMap::new();
    headers.insert("X-API-Key".to_string(), "secret".to_string());
    headers.insert("X-Custom".to_string(), "value".to_string());

    let config = McpServerConfig::new("test", "http://localhost:8080").with_headers(headers);
    let client = McpClient::new_with_config(config.clone());

    assert_eq!(client.server_name(), "test");
    assert_eq!(client.server_url(), "http://localhost:8080");
    assert_eq!(client.custom_headers.len(), 2);
    assert_eq!(client.custom_headers.get("X-API-Key").unwrap(), "secret");
    assert!(client.server_config.is_some());
}

#[test]
fn test_new_with_config_no_headers() {
    let config = McpServerConfig::new("bare", "http://localhost:9090");
    let client = McpClient::new_with_config(config);

    assert_eq!(client.server_name(), "bare");
    assert!(client.custom_headers.is_empty());
    assert!(client.secrets.is_none());
    assert!(client.session_manager.is_none());
}

#[test]
fn test_next_request_id_monotonically_increasing() {
    let client = McpClient::new("http://localhost:1234");
    assert_eq!(client.next_request_id(), 1);
    assert_eq!(client.next_request_id(), 2);
    assert_eq!(client.next_request_id(), 3);
}

#[test]
fn test_mcp_tool_requires_approval_destructive() {
    use crate::tools::mcp::protocol::{McpTool, McpToolAnnotations};
    let tool = McpTool {
        name: "delete_all".to_string(),
        description: "Deletes everything".to_string(),
        input_schema: serde_json::json!({"type": "object"}),
        annotations: Some(McpToolAnnotations {
            destructive_hint: true,
            side_effects_hint: false,
            read_only_hint: false,
            execution_time_hint: None,
        }),
    };
    assert!(tool.requires_approval());
}

#[test]
fn test_mcp_tool_no_approval_when_not_destructive() {
    use crate::tools::mcp::protocol::{McpTool, McpToolAnnotations};
    let tool = McpTool {
        name: "read_data".to_string(),
        description: "Reads data".to_string(),
        input_schema: serde_json::json!({"type": "object"}),
        annotations: Some(McpToolAnnotations {
            destructive_hint: false,
            side_effects_hint: true,
            read_only_hint: false,
            execution_time_hint: None,
        }),
    };
    assert!(!tool.requires_approval());
}

#[test]
fn test_mcp_tool_no_approval_when_no_annotations() {
    use crate::tools::mcp::protocol::McpTool;
    let tool = McpTool {
        name: "simple_tool".to_string(),
        description: "A simple tool".to_string(),
        input_schema: serde_json::json!({"type": "object"}),
        annotations: None,
    };
    assert!(!tool.requires_approval());
}

/// Mock transport for testing transport abstraction behavior.
struct MockTransport {
    supports_http: bool,
    responses: std::sync::Mutex<Vec<McpResponse>>,
    recorded_headers: std::sync::Mutex<Vec<HashMap<String, String>>>,
}

impl MockTransport {
    fn new(supports_http: bool, responses: Vec<McpResponse>) -> Self {
        Self {
            supports_http,
            responses: std::sync::Mutex::new(responses),
            recorded_headers: std::sync::Mutex::new(Vec::new()),
        }
    }
    fn recorded_headers(&self) -> Vec<HashMap<String, String>> {
        self.recorded_headers.lock().unwrap().clone()
    }
}

impl NativeMcpTransport for MockTransport {
    async fn send(
        &self,
        _request: &McpRequest,
        headers: &HashMap<String, String>,
    ) -> Result<McpResponse, ToolError> {
        self.recorded_headers.lock().unwrap().push(headers.clone());
        let mut responses = self.responses.lock().unwrap();
        if responses.is_empty() {
            return Err(ToolError::ExternalService(
                "No more mock responses".to_string(),
            ));
        }
        Ok(responses.remove(0))
    }
    async fn shutdown(&self) -> Result<(), ToolError> {
        Ok(())
    }
    fn supports_http_features(&self) -> bool {
        self.supports_http
    }
}

#[tokio::test]
async fn test_non_http_transport_skips_401_retry() {
    let response = McpResponse {
        jsonrpc: "2.0".to_string(),
        id: Some(1),
        result: Some(serde_json::json!({"tools": []})),
        error: None,
    };
    let transport = Arc::new(MockTransport::new(false, vec![response]));
    let client =
        McpClient::new_with_transport("test-stdio", transport.clone(), None, None, "default", None);
    let result = client.list_tools().await;
    assert!(result.is_ok());
    assert_eq!(result.unwrap().len(), 0);
    let headers = transport.recorded_headers();
    assert_eq!(headers.len(), 1);
    assert!(!headers[0].contains_key("Authorization"));
    assert!(!headers[0].contains_key("Mcp-Session-Id"));
}

#[tokio::test]
async fn test_transport_supports_http_features_accessor() {
    let http_transport = HttpMcpTransport::new("http://localhost:8080", "test");
    assert!(McpTransport::supports_http_features(&http_transport));
    let mock_non_http = MockTransport::new(false, vec![]);
    assert!(!McpTransport::supports_http_features(&mock_non_http));
}

#[test]
fn test_strip_top_level_nulls_removes_null_fields() {
    let input = serde_json::json!({
        "query": "search term",
        "sort": null,
        "filter": null,
        "page_size": 10
    });
    let result = strip_top_level_nulls(input);
    let obj = result.as_object().unwrap();
    assert_eq!(obj.len(), 2);
    assert_eq!(obj["query"], "search term");
    assert_eq!(obj["page_size"], 10);
    assert!(!obj.contains_key("sort"));
    assert!(!obj.contains_key("filter"));
}

#[test]
fn test_strip_top_level_nulls_preserves_non_objects() {
    let input = serde_json::json!("just a string");
    let result = strip_top_level_nulls(input.clone());
    assert_eq!(result, input);
}

#[test]
fn test_strip_top_level_nulls_preserves_nested_nulls() {
    let input = serde_json::json!({
        "outer": { "inner": null },
        "top_null": null
    });
    let result = strip_top_level_nulls(input);
    let obj = result.as_object().unwrap();
    assert_eq!(obj.len(), 1);
    assert!(obj["outer"]["inner"].is_null());
}
