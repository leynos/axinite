//! Tests for `ModelInfo` serde aliases and request serialization.

use super::super::wire::*;
use super::super::*;

// -- ModelInfo serde alias tests ------------------------------------------

#[test]
fn test_model_info_deserialize_with_name_field() {
    let json = r#"{"name": "claude-3-5-sonnet"}"#;
    let info: ModelInfo = serde_json::from_str(json).unwrap();
    assert_eq!(info.name, "claude-3-5-sonnet");
    assert!(info.provider.is_none());
}

#[test]
fn test_model_info_deserialize_with_id_alias() {
    let json = r#"{"id": "gpt-4o", "provider": "openai"}"#;
    let info: ModelInfo = serde_json::from_str(json).unwrap();
    assert_eq!(info.name, "gpt-4o");
    assert_eq!(info.provider, Some("openai".to_string()));
}

#[test]
fn test_model_info_deserialize_with_model_alias() {
    let json = r#"{"model": "llama-3.1-70b"}"#;
    let info: ModelInfo = serde_json::from_str(json).unwrap();
    assert_eq!(info.name, "llama-3.1-70b");
}

#[test]
fn test_model_info_roundtrip_serializes_as_name() {
    let info = ModelInfo {
        name: "test-model".to_string(),
        provider: Some("nearai".to_string()),
    };
    let json = serde_json::to_value(&info).unwrap();
    // Serialization always uses the field name "name", not the aliases
    assert_eq!(json["name"], "test-model");
    assert_eq!(json["provider"], "nearai");
    assert!(json.get("id").is_none());
    assert!(json.get("model").is_none());
}

// -- ChatCompletionRequest serialization ----------------------------------

#[test]
fn test_request_serialization_minimal() {
    let req = ChatCompletionRequest {
        model: "gpt-4o".to_string(),
        messages: vec![ChatCompletionMessage {
            role: "user".to_string(),
            content: Some(MessageContent::Text("Hello".to_string())),
            tool_call_id: None,
            name: None,
            tool_calls: None,
        }],
        temperature: None,
        max_tokens: None,
        tools: None,
        tool_choice: None,
    };
    let json = serde_json::to_value(&req).unwrap();
    assert_eq!(json["model"], "gpt-4o");
    assert_eq!(json["messages"][0]["role"], "user");
    assert_eq!(json["messages"][0]["content"], "Hello");
    // Optional fields should be absent, not null
    assert!(json.get("temperature").is_none());
    assert!(json.get("max_tokens").is_none());
    assert!(json.get("tools").is_none());
    assert!(json.get("tool_choice").is_none());
}

#[test]
fn test_request_serialization_with_tools() {
    let req = ChatCompletionRequest {
        model: "gpt-4o".to_string(),
        messages: vec![],
        temperature: Some(0.7),
        max_tokens: Some(1024),
        tools: Some(vec![ChatCompletionTool {
            tool_type: "function".to_string(),
            function: ChatCompletionFunction {
                name: "get_weather".to_string(),
                description: Some("Get the weather".to_string()),
                parameters: Some(serde_json::json!({
                    "type": "object",
                    "properties": {
                        "city": {"type": "string"}
                    }
                })),
            },
        }]),
        tool_choice: Some("auto".to_string()),
    };
    let json = serde_json::to_value(&req).unwrap();
    // f32 precision: 0.7f32 serializes as 0.699999988... in JSON
    let temp = json["temperature"].as_f64().unwrap();
    assert!(
        (temp - 0.7).abs() < 0.001,
        "temperature should be ~0.7, got {temp}"
    );
    assert_eq!(json["max_tokens"], 1024);
    assert_eq!(json["tool_choice"], "auto");
    // Tool uses "type" key (via rename), not "tool_type"
    assert_eq!(json["tools"][0]["type"], "function");
    assert_eq!(json["tools"][0]["function"]["name"], "get_weather");
}

#[test]
fn test_request_omits_null_content_on_assistant_messages() {
    // When an assistant message has tool_calls but no content, content
    // should serialize as absent (skip_serializing_if) not "content": null.
    let msg = ChatCompletionMessage {
        role: "assistant".to_string(),
        content: None,
        tool_call_id: None,
        name: None,
        tool_calls: Some(vec![ChatCompletionToolCall {
            id: "call_1".to_string(),
            call_type: "function".to_string(),
            function: ChatCompletionToolCallFunction {
                name: "echo".to_string(),
                arguments: "{}".to_string(),
            },
        }]),
    };
    let json = serde_json::to_value(&msg).unwrap();
    assert!(
        json.get("content").is_none(),
        "content should be omitted when None"
    );
    assert!(json.get("tool_call_id").is_none());
    assert!(json.get("name").is_none());
    assert!(json["tool_calls"].is_array());
}
