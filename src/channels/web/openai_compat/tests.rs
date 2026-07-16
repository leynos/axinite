//! Unit tests for OpenAI-compatible role and finish-reason mapping.

use super::convert::{
    convert_messages, convert_tool_calls_to_openai, convert_tools, finish_reason_str,
    normalize_tool_choice, parse_role, parse_stop, validate_model_name,
};
use super::types::{
    OpenAiChatRequest, OpenAiChatResponse, OpenAiChoice, OpenAiFunction, OpenAiMessage, OpenAiTool,
    OpenAiUsage,
};
use crate::llm::{FinishReason, Role, ToolCall};

#[test]
fn test_parse_role() {
    assert_eq!(parse_role("system").unwrap(), Role::System);
    assert_eq!(parse_role("user").unwrap(), Role::User);
    assert_eq!(parse_role("assistant").unwrap(), Role::Assistant);
    assert_eq!(parse_role("tool").unwrap(), Role::Tool);
}

#[test]
fn test_parse_role_unknown_rejected() {
    let err = parse_role("unknown").unwrap_err();
    assert!(err.contains("Unknown role"));
    assert!(err.contains("unknown"));
}

#[test]
fn test_finish_reason_str() {
    assert_eq!(finish_reason_str(FinishReason::Stop), "stop");
    assert_eq!(finish_reason_str(FinishReason::Length), "length");
    assert_eq!(finish_reason_str(FinishReason::ToolUse), "tool_calls");
    assert_eq!(
        finish_reason_str(FinishReason::ContentFilter),
        "content_filter"
    );
    assert_eq!(finish_reason_str(FinishReason::Unknown), "stop");
}

#[test]
fn test_convert_messages_basic() {
    let msgs = vec![
        OpenAiMessage {
            role: "system".to_string(),
            content: Some("You are helpful.".to_string()),
            name: None,
            tool_call_id: None,
            tool_calls: None,
        },
        OpenAiMessage {
            role: "user".to_string(),
            content: Some("Hello".to_string()),
            name: None,
            tool_call_id: None,
            tool_calls: None,
        },
    ];

    let converted = convert_messages(&msgs).unwrap();
    assert_eq!(converted.len(), 2);
    assert_eq!(converted[0].role, Role::System);
    assert_eq!(converted[0].content, "You are helpful.");
    assert_eq!(converted[1].role, Role::User);
    assert_eq!(converted[1].content, "Hello");
}

#[test]
fn test_convert_messages_with_tool_results() {
    let msgs = vec![OpenAiMessage {
        role: "tool".to_string(),
        content: Some("42".to_string()),
        name: Some("calculator".to_string()),
        tool_call_id: Some("call_123".to_string()),
        tool_calls: None,
    }];

    let converted = convert_messages(&msgs).unwrap();
    assert_eq!(converted.len(), 1);
    assert_eq!(converted[0].role, Role::Tool);
    assert_eq!(converted[0].content, "42");
    assert_eq!(converted[0].tool_call_id.as_deref(), Some("call_123"));
    assert_eq!(converted[0].name.as_deref(), Some("calculator"));
}

#[test]
fn test_convert_tools() {
    let tools = vec![OpenAiTool {
        tool_type: "function".to_string(),
        function: OpenAiFunction {
            name: "get_weather".to_string(),
            description: Some("Get weather for a location".to_string()),
            parameters: Some(serde_json::json!({
                "type": "object",
                "properties": {
                    "location": { "type": "string" }
                },
                "required": ["location"]
            })),
        },
    }];

    let converted = convert_tools(&tools);
    assert_eq!(converted.len(), 1);
    assert_eq!(converted[0].name, "get_weather");
    assert_eq!(converted[0].description, "Get weather for a location");
}

#[test]
fn test_convert_tool_calls_to_openai() {
    let calls = vec![ToolCall {
        id: "call_abc".to_string(),
        name: "search".to_string(),
        arguments: serde_json::json!({"query": "rust"}),
    }];

    let converted = convert_tool_calls_to_openai(&calls);
    assert_eq!(converted.len(), 1);
    assert_eq!(converted[0].id, "call_abc");
    assert_eq!(converted[0].call_type, "function");
    assert_eq!(converted[0].function.name, "search");
    assert!(converted[0].function.arguments.contains("rust"));
}

#[test]
fn test_normalize_tool_choice() {
    // String variant
    let v = serde_json::json!("auto");
    assert_eq!(normalize_tool_choice(&v), Some("auto".to_string()));

    // Object with function
    let v = serde_json::json!({"type": "function", "function": {"name": "foo"}});
    assert_eq!(normalize_tool_choice(&v), Some("required".to_string()));

    // Object with type only
    let v = serde_json::json!({"type": "none"});
    assert_eq!(normalize_tool_choice(&v), Some("none".to_string()));

    // Null
    let v = serde_json::Value::Null;
    assert_eq!(normalize_tool_choice(&v), None);
}

#[test]
fn test_openai_request_deserialize_minimal() {
    let json = r#"{"model":"gpt-4","messages":[{"role":"user","content":"Hi"}]}"#;
    let req: OpenAiChatRequest = serde_json::from_str(json).unwrap();
    assert_eq!(req.model, "gpt-4");
    assert_eq!(req.messages.len(), 1);
    assert_eq!(req.stream, None);
    assert_eq!(req.temperature, None);
}

#[test]
fn test_openai_request_deserialize_streaming() {
    let json = r#"{"model":"gpt-4","messages":[{"role":"user","content":"Hi"}],"stream":true,"temperature":0.7}"#;
    let req: OpenAiChatRequest = serde_json::from_str(json).unwrap();
    assert_eq!(req.stream, Some(true));
    assert_eq!(req.temperature, Some(0.7));
}

#[test]
fn test_openai_response_serialize() {
    let resp = OpenAiChatResponse {
        id: "chatcmpl-test".to_string(),
        object: "chat.completion",
        created: 1234567890,
        model: "test-model".to_string(),
        choices: vec![OpenAiChoice {
            index: 0,
            message: OpenAiMessage {
                role: "assistant".to_string(),
                content: Some("Hello!".to_string()),
                name: None,
                tool_call_id: None,
                tool_calls: None,
            },
            finish_reason: "stop".to_string(),
        }],
        usage: OpenAiUsage {
            prompt_tokens: 10,
            completion_tokens: 5,
            total_tokens: 15,
        },
    };

    let json = serde_json::to_value(&resp).unwrap();
    assert_eq!(json["object"], "chat.completion");
    assert_eq!(json["choices"][0]["finish_reason"], "stop");
    assert_eq!(json["choices"][0]["message"]["content"], "Hello!");
    assert_eq!(json["usage"]["total_tokens"], 15);
}

#[test]
fn test_openai_message_with_null_content() {
    let json = r#"{"role":"assistant","content":null,"tool_calls":[{"id":"call_1","type":"function","function":{"name":"search","arguments":"{\"q\":\"test\"}"}}]}"#;
    let msg: OpenAiMessage = serde_json::from_str(json).unwrap();
    assert_eq!(msg.role, "assistant");
    assert!(msg.content.is_none());
    assert!(msg.tool_calls.is_some());
    assert_eq!(msg.tool_calls.as_ref().unwrap().len(), 1);
}

#[test]
fn test_convert_messages_unknown_role_rejected() {
    let msgs = vec![OpenAiMessage {
        role: "moderator".to_string(),
        content: Some("Hi".to_string()),
        name: None,
        tool_call_id: None,
        tool_calls: None,
    }];
    let err = convert_messages(&msgs).unwrap_err();
    assert!(err.contains("messages[0]"));
    assert!(err.contains("Unknown role"));
}

#[test]
fn test_convert_messages_tool_missing_fields() {
    // Missing tool_call_id
    let msgs = vec![OpenAiMessage {
        role: "tool".to_string(),
        content: Some("result".to_string()),
        name: Some("calc".to_string()),
        tool_call_id: None,
        tool_calls: None,
    }];
    let err = convert_messages(&msgs).unwrap_err();
    assert!(err.contains("tool_call_id"));

    // Missing name
    let msgs = vec![OpenAiMessage {
        role: "tool".to_string(),
        content: Some("result".to_string()),
        name: None,
        tool_call_id: Some("call_1".to_string()),
        tool_calls: None,
    }];
    let err = convert_messages(&msgs).unwrap_err();
    assert!(err.contains("'name'"));
}

#[test]
fn test_parse_stop_string() {
    let v = serde_json::json!("STOP");
    assert_eq!(parse_stop(&v), Some(vec!["STOP".to_string()]));
}

#[test]
fn test_parse_stop_array() {
    let v = serde_json::json!(["STOP", "END"]);
    assert_eq!(
        parse_stop(&v),
        Some(vec!["STOP".to_string(), "END".to_string()])
    );
}

#[test]
fn test_parse_stop_null() {
    let v = serde_json::Value::Null;
    assert_eq!(parse_stop(&v), None);
}

#[test]
fn test_validate_model_name_rejects_leading_or_trailing_whitespace() {
    let err = validate_model_name(" gpt-4").unwrap_err();
    assert!(err.contains("leading or trailing whitespace"));

    let err = validate_model_name("gpt-4 ").unwrap_err();
    assert!(err.contains("leading or trailing whitespace"));
}

#[test]
fn test_validate_model_name_accepts_normal_name() {
    assert!(validate_model_name("gpt-4").is_ok());
}
