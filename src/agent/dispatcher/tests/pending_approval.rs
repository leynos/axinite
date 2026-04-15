//! PendingApproval serialization tests.

use super::*;

#[test]
fn test_pending_approval_serialization_backcompat_without_deferred_calls() {
    // PendingApproval from before the deferred_tool_calls field was added
    // should deserialize with an empty vec (via #[serde(default)]).
    let json = serde_json::json!({
        "request_id": uuid::Uuid::new_v4(),
        "tool_name": "http",
        "parameters": {"url": "https://example.com", "method": "GET"},
        "description": "Make HTTP request",
        "tool_call_id": "call_123",
        "context_messages": [{"role": "user", "content": "go"}]
    })
    .to_string();

    let parsed: crate::agent::session::PendingApproval =
        serde_json::from_str(&json).expect("should deserialize without deferred_tool_calls");

    assert!(parsed.deferred_tool_calls.is_empty());
    assert_eq!(parsed.tool_name, "http");
    assert_eq!(parsed.tool_call_id, "call_123");
}

#[test]
fn test_pending_approval_serialization_roundtrip_with_deferred_calls() {
    let pending = crate::agent::session::PendingApproval {
        request_id: uuid::Uuid::new_v4(),
        tool_name: "shell".to_string(),
        parameters: serde_json::json!({"command": "echo hi"}),
        display_parameters: serde_json::json!({"command": "echo hi"}),
        description: "Run shell command".to_string(),
        tool_call_id: "call_1".to_string(),
        context_messages: vec![],
        deferred_tool_calls: vec![
            ToolCall {
                id: "call_2".to_string(),
                name: "http".to_string(),
                arguments: serde_json::json!({"url": "https://example.com"}),
            },
            ToolCall {
                id: "call_3".to_string(),
                name: "echo".to_string(),
                arguments: serde_json::json!({"message": "done"}),
            },
        ],
        user_timezone: None,
    };

    let json = serde_json::to_string(&pending).expect("serialize");
    let parsed: crate::agent::session::PendingApproval =
        serde_json::from_str(&json).expect("deserialize");

    assert_eq!(parsed.deferred_tool_calls.len(), 2);
    assert_eq!(parsed.deferred_tool_calls[0].name, "http");
    assert_eq!(parsed.deferred_tool_calls[1].name, "echo");
}
