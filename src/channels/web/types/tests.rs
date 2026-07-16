//! Unit tests for WebSocket message serialization and
//! deserialization.

use uuid::Uuid;

use super::*;

// ---- WsClientMessage deserialization tests ----

#[test]
fn test_ws_client_message_parse() {
    let json = r#"{"type":"message","content":"hello","thread_id":"t1"}"#;
    let msg: WsClientMessage = serde_json::from_str(json).unwrap();
    match msg {
        WsClientMessage::Message {
            content, thread_id, ..
        } => {
            assert_eq!(content, "hello");
            assert_eq!(thread_id.as_deref(), Some("t1"));
        }
        _ => panic!("Expected Message variant"),
    }
}

#[test]
fn test_ws_client_message_no_thread() {
    let json = r#"{"type":"message","content":"hi"}"#;
    let msg: WsClientMessage = serde_json::from_str(json).unwrap();
    match msg {
        WsClientMessage::Message {
            content, thread_id, ..
        } => {
            assert_eq!(content, "hi");
            assert!(thread_id.is_none());
        }
        _ => panic!("Expected Message variant"),
    }
}

#[test]
fn test_ws_client_approval_parse() {
    let json = r#"{"type":"approval","request_id":"abc-123","action":"approve","thread_id":"t1"}"#;
    let msg: WsClientMessage = serde_json::from_str(json).unwrap();
    match msg {
        WsClientMessage::Approval {
            request_id,
            action,
            thread_id,
        } => {
            assert_eq!(request_id, "abc-123");
            assert_eq!(action, "approve");
            assert_eq!(thread_id.as_deref(), Some("t1"));
        }
        _ => panic!("Expected Approval variant"),
    }
}

#[test]
fn test_ws_client_approval_parse_no_thread() {
    let json = r#"{"type":"approval","request_id":"abc-123","action":"deny"}"#;
    let msg: WsClientMessage = serde_json::from_str(json).unwrap();
    match msg {
        WsClientMessage::Approval {
            request_id,
            action,
            thread_id,
        } => {
            assert_eq!(request_id, "abc-123");
            assert_eq!(action, "deny");
            assert!(thread_id.is_none());
        }
        _ => panic!("Expected Approval variant"),
    }
}

#[test]
fn test_ws_client_ping_parse() {
    let json = r#"{"type":"ping"}"#;
    let msg: WsClientMessage = serde_json::from_str(json).unwrap();
    assert!(matches!(msg, WsClientMessage::Ping));
}

#[test]
fn test_ws_client_unknown_type_fails() {
    let json = r#"{"type":"unknown"}"#;
    let result: Result<WsClientMessage, _> = serde_json::from_str(json);
    assert!(result.is_err());
}

// ---- WsServerMessage serialization tests ----

#[test]
fn test_ws_server_pong_serialize() {
    let msg = WsServerMessage::Pong;
    let json = serde_json::to_string(&msg).unwrap();
    assert_eq!(json, r#"{"type":"pong"}"#);
}

#[test]
fn test_ws_server_error_serialize() {
    let msg = WsServerMessage::Error {
        message: "bad request".to_string(),
    };
    let json = serde_json::to_string(&msg).unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed["type"], "error");
    assert_eq!(parsed["message"], "bad request");
}

#[test]
fn test_ws_server_from_sse_response() {
    let sse = SseEvent::Response {
        content: "hello".to_string(),
        thread_id: "t1".to_string(),
    };
    let ws = WsServerMessage::from_sse_event(&sse);
    match ws {
        WsServerMessage::Event { event_type, data } => {
            assert_eq!(event_type, "response");
            assert_eq!(data["content"], "hello");
            assert_eq!(data["thread_id"], "t1");
        }
        _ => panic!("Expected Event variant"),
    }
}

#[test]
fn test_ws_server_from_sse_thinking() {
    let sse = SseEvent::Thinking {
        message: "reasoning...".to_string(),
        thread_id: None,
    };
    let ws = WsServerMessage::from_sse_event(&sse);
    match ws {
        WsServerMessage::Event { event_type, data } => {
            assert_eq!(event_type, "thinking");
            assert_eq!(data["message"], "reasoning...");
        }
        _ => panic!("Expected Event variant"),
    }
}

#[test]
fn test_ws_server_from_sse_approval_needed() {
    let sse = SseEvent::ApprovalNeeded {
        request_id: "r1".to_string(),
        tool_name: "shell".to_string(),
        description: "Run ls".to_string(),
        parameters: "{}".to_string(),
        thread_id: Some("t1".to_string()),
    };
    let ws = WsServerMessage::from_sse_event(&sse);
    match ws {
        WsServerMessage::Event { event_type, data } => {
            assert_eq!(event_type, "approval_needed");
            assert_eq!(data["tool_name"], "shell");
            assert_eq!(data["thread_id"], "t1");
        }
        _ => panic!("Expected Event variant"),
    }
}

#[test]
fn test_ws_server_from_sse_heartbeat() {
    let sse = SseEvent::Heartbeat;
    let ws = WsServerMessage::from_sse_event(&sse);
    match ws {
        WsServerMessage::Event { event_type, .. } => {
            assert_eq!(event_type, "heartbeat");
        }
        _ => panic!("Expected Event variant"),
    }
}

// ---- Auth type tests ----

#[test]
fn test_ws_client_auth_token_parse() {
    let json = r#"{"type":"auth_token","extension_name":"notion","token":"sk-123"}"#;
    let msg: WsClientMessage = serde_json::from_str(json).unwrap();
    match msg {
        WsClientMessage::AuthToken {
            extension_name,
            token,
        } => {
            assert_eq!(extension_name, "notion");
            assert_eq!(token, "sk-123");
        }
        _ => panic!("Expected AuthToken variant"),
    }
}

#[test]
fn test_ws_client_auth_cancel_parse() {
    let json = r#"{"type":"auth_cancel","extension_name":"notion"}"#;
    let msg: WsClientMessage = serde_json::from_str(json).unwrap();
    match msg {
        WsClientMessage::AuthCancel { extension_name } => {
            assert_eq!(extension_name, "notion");
        }
        _ => panic!("Expected AuthCancel variant"),
    }
}

#[test]
fn test_sse_auth_required_serialize() {
    let event = SseEvent::AuthRequired {
        extension_name: "notion".to_string(),
        instructions: Some("Get your token from...".to_string()),
        auth_url: None,
        setup_url: Some("https://notion.so/integrations".to_string()),
    };
    let json = serde_json::to_string(&event).unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed["type"], "auth_required");
    assert_eq!(parsed["extension_name"], "notion");
    assert_eq!(parsed["instructions"], "Get your token from...");
    assert!(parsed.get("auth_url").is_none());
    assert_eq!(parsed["setup_url"], "https://notion.so/integrations");
}

#[test]
fn test_sse_auth_completed_serialize() {
    let event = SseEvent::AuthCompleted {
        extension_name: "notion".to_string(),
        success: true,
        message: "notion authenticated (3 tools loaded)".to_string(),
    };
    let json = serde_json::to_string(&event).unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed["type"], "auth_completed");
    assert_eq!(parsed["extension_name"], "notion");
    assert_eq!(parsed["success"], true);
}

#[test]
fn test_ws_server_from_sse_auth_required() {
    let sse = SseEvent::AuthRequired {
        extension_name: "openai".to_string(),
        instructions: Some("Enter API key".to_string()),
        auth_url: None,
        setup_url: None,
    };
    let ws = WsServerMessage::from_sse_event(&sse);
    match ws {
        WsServerMessage::Event { event_type, data } => {
            assert_eq!(event_type, "auth_required");
            assert_eq!(data["extension_name"], "openai");
        }
        _ => panic!("Expected Event variant"),
    }
}

#[test]
fn test_ws_server_from_sse_auth_completed() {
    let sse = SseEvent::AuthCompleted {
        extension_name: "slack".to_string(),
        success: false,
        message: "Invalid token".to_string(),
    };
    let ws = WsServerMessage::from_sse_event(&sse);
    match ws {
        WsServerMessage::Event { event_type, data } => {
            assert_eq!(event_type, "auth_completed");
            assert_eq!(data["success"], false);
        }
        _ => panic!("Expected Event variant"),
    }
}

#[test]
fn test_auth_token_request_deserialize() {
    let json = r#"{"extension_name":"telegram","token":"bot12345"}"#;
    let req: AuthTokenRequest = serde_json::from_str(json).unwrap();
    assert_eq!(req.extension_name, "telegram");
    assert_eq!(req.token, "bot12345");
}

#[test]
fn test_auth_cancel_request_deserialize() {
    let json = r#"{"extension_name":"telegram"}"#;
    let req: AuthCancelRequest = serde_json::from_str(json).unwrap();
    assert_eq!(req.extension_name, "telegram");
}

// ---- ThreadInfo channel field tests ----

#[test]
fn test_thread_info_channel_serialized() {
    let info = ThreadInfo {
        id: Uuid::nil(),
        state: "Idle".to_string(),
        turn_count: 0,
        created_at: "2026-01-01T00:00:00Z".to_string(),
        updated_at: "2026-01-01T00:00:00Z".to_string(),
        title: None,
        thread_type: None,
        channel: Some("telegram".to_string()),
    };
    let json = serde_json::to_string(&info).unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed["channel"], "telegram");
}

#[test]
fn test_thread_info_channel_omitted_when_none() {
    let info = ThreadInfo {
        id: Uuid::nil(),
        state: "Idle".to_string(),
        turn_count: 0,
        created_at: "2026-01-01T00:00:00Z".to_string(),
        updated_at: "2026-01-01T00:00:00Z".to_string(),
        title: None,
        thread_type: None,
        channel: None,
    };
    let json = serde_json::to_string(&info).unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
    assert!(parsed.get("channel").is_none());
}
