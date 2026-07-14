//! Tests for JSON-RPC message shapes: requests, responses, errors,
//! initialization results, and tool-call content blocks.

use super::super::*;

#[test]
fn test_initialize_request() {
    let req = McpRequest::initialize(42);
    assert_eq!(req.jsonrpc, "2.0");
    assert_eq!(req.id, Some(42));
    assert_eq!(req.method, "initialize");

    let params = req.params.expect("initialize must have params");
    assert_eq!(params["protocolVersion"], PROTOCOL_VERSION);
    assert!(params["capabilities"].is_object());
    assert!(params["capabilities"]["roots"].is_object());
    assert!(params["capabilities"]["sampling"].is_object());
    assert_eq!(params["clientInfo"]["name"], "ironclaw");
    assert!(params["clientInfo"]["version"].is_string());
}

#[test]
fn test_initialized_notification() {
    let req = McpRequest::initialized_notification();
    assert_eq!(req.jsonrpc, "2.0");
    assert_eq!(req.method, "notifications/initialized");
    assert!(req.params.is_none());
}

#[test]
fn test_call_tool_request() {
    let args = serde_json::json!({"query": "rust async"});
    let req = McpRequest::call_tool(7, "search", args.clone());
    assert_eq!(req.id, Some(7));
    assert_eq!(req.method, "tools/call");

    let params = req.params.expect("call_tool must have params");
    assert_eq!(params["name"], "search");
    assert_eq!(params["arguments"], args);
}

#[test]
fn test_mcp_response_deserialize_success() {
    let json = serde_json::json!({
        "jsonrpc": "2.0",
        "id": 1,
        "result": { "tools": [] }
    });
    let resp: McpResponse = serde_json::from_value(json).expect("deserialize");
    assert_eq!(resp.id, Some(1));
    assert!(resp.result.is_some());
    assert!(resp.error.is_none());
}

#[test]
fn test_mcp_response_deserialize_error() {
    let json = serde_json::json!({
        "jsonrpc": "2.0",
        "id": 2,
        "error": {
            "code": -32601,
            "message": "Method not found"
        }
    });
    let resp: McpResponse = serde_json::from_value(json).expect("deserialize");
    assert!(resp.result.is_none());
    let err = resp.error.expect("should have error");
    assert_eq!(err.code, -32601);
    assert_eq!(err.message, "Method not found");
    assert!(err.data.is_none());
}

#[test]
fn test_mcp_error_roundtrip() {
    let err = McpError {
        code: -32600,
        message: "Invalid Request".to_string(),
        data: Some(serde_json::json!({"detail": "missing field"})),
    };
    let serialized = serde_json::to_string(&err).expect("serialize");
    let deserialized: McpError = serde_json::from_str(&serialized).expect("deserialize");
    assert_eq!(deserialized.code, err.code);
    assert_eq!(deserialized.message, err.message);
    assert_eq!(deserialized.data, err.data);
}

#[test]
fn test_initialize_result_full() {
    let json = serde_json::json!({
        "protocolVersion": "2024-11-05",
        "capabilities": {
            "tools": { "listChanged": true },
            "resources": { "subscribe": true, "listChanged": false },
            "prompts": { "listChanged": true },
            "logging": {}
        },
        "serverInfo": {
            "name": "test-server",
            "version": "1.2.3"
        },
        "instructions": "Use this server for testing."
    });
    let result: InitializeResult = serde_json::from_value(json).expect("deserialize");
    assert_eq!(result.protocol_version.as_deref(), Some("2024-11-05"));

    let tools_cap = result.capabilities.tools.expect("has tools capability");
    assert!(tools_cap.list_changed);

    let res_cap = result
        .capabilities
        .resources
        .expect("has resources capability");
    assert!(res_cap.subscribe);
    assert!(!res_cap.list_changed);

    let prompts_cap = result.capabilities.prompts.expect("has prompts capability");
    assert!(prompts_cap.list_changed);

    assert!(result.capabilities.logging.is_some());

    let info = result.server_info.expect("has server info");
    assert_eq!(info.name, "test-server");
    assert_eq!(info.version.as_deref(), Some("1.2.3"));
    assert_eq!(
        result.instructions.as_deref(),
        Some("Use this server for testing.")
    );
}

#[test]
fn test_content_block_as_text() {
    let text_block = ContentBlock::Text {
        text: "hello".to_string(),
    };
    assert_eq!(text_block.as_text(), Some("hello"));

    let image_block = ContentBlock::Image {
        data: "base64data".to_string(),
        mime_type: "image/png".to_string(),
    };
    assert!(image_block.as_text().is_none());

    let resource_block = ContentBlock::Resource {
        uri: "file:///tmp/a.txt".to_string(),
        mime_type: Some("text/plain".to_string()),
        text: Some("content".to_string()),
    };
    assert!(resource_block.as_text().is_none());
}

#[test]
fn test_content_block_serde_tagged_union() {
    let text_block = ContentBlock::Text {
        text: "hi".to_string(),
    };
    let json = serde_json::to_value(&text_block).expect("serialize");
    assert_eq!(json["type"], "text");
    assert_eq!(json["text"], "hi");

    let image_block = ContentBlock::Image {
        data: "abc".to_string(),
        mime_type: "image/jpeg".to_string(),
    };
    let json = serde_json::to_value(&image_block).expect("serialize");
    assert_eq!(json["type"], "image");
    assert_eq!(json["data"], "abc");
    assert_eq!(json["mime_type"], "image/jpeg");

    let resource_block = ContentBlock::Resource {
        uri: "file:///x".to_string(),
        mime_type: None,
        text: None,
    };
    let json = serde_json::to_value(&resource_block).expect("serialize");
    assert_eq!(json["type"], "resource");
    assert_eq!(json["uri"], "file:///x");
}

#[test]
fn test_call_tool_result_is_error() {
    let success: CallToolResult = serde_json::from_value(serde_json::json!({
        "content": [{"type": "text", "text": "done"}],
        "is_error": false
    }))
    .expect("deserialize");
    assert!(!success.is_error);
    assert_eq!(success.content.len(), 1);

    let failure: CallToolResult = serde_json::from_value(serde_json::json!({
        "content": [{"type": "text", "text": "boom"}],
        "is_error": true
    }))
    .expect("deserialize");
    assert!(failure.is_error);
}

#[test]
fn test_call_tool_result_is_error_defaults_false() {
    let result: CallToolResult = serde_json::from_value(serde_json::json!({
        "content": []
    }))
    .expect("deserialize");
    assert!(!result.is_error);
}

#[test]
fn test_notification_serializes_without_id_field() {
    // JSON-RPC 2.0 spec: notifications MUST NOT have an "id" field.
    let notif = McpRequest::initialized_notification();
    let json = serde_json::to_value(&notif).expect("serialize notification");
    assert!(
        json.get("id").is_none(),
        "notifications must not contain an 'id' field per JSON-RPC 2.0 spec"
    );
    assert_eq!(json.get("method").unwrap(), "notifications/initialized");
}

#[test]
fn test_response_with_string_id() {
    // Some MCP servers return id as a string instead of a number.
    let json = serde_json::json!({
        "jsonrpc": "2.0",
        "id": "42",
        "result": {}
    });
    let resp: McpResponse = serde_json::from_value(json).expect("deserialize string id");
    assert_eq!(resp.id, Some(42));
}

#[test]
fn test_response_with_null_id() {
    // JSON-RPC error responses may have a null id.
    let json = serde_json::json!({
        "jsonrpc": "2.0",
        "id": null,
        "error": { "code": -32700, "message": "Parse error" }
    });
    let resp: McpResponse = serde_json::from_value(json).expect("deserialize null id");
    assert_eq!(resp.id, None);
}

#[test]
fn test_response_with_non_numeric_string_id() {
    // Some servers send non-numeric string ids — these should parse as None.
    let json = serde_json::json!({
        "jsonrpc": "2.0",
        "id": "not-a-number",
        "result": {}
    });
    let resp: McpResponse =
        serde_json::from_value(json).expect("deserialize non-numeric string id");
    assert_eq!(resp.id, None);
}
