//! Tests for response deserialization, reasoning-content fallback, and
//! usage parsing.

use super::super::wire::*;
use crate::llm::provider::ToolCall;

/// Regression: reasoning_content must NOT leak into tool-call responses.
#[test]
fn test_reasoning_content_not_leaked_into_tool_call_response() {
    let response: ChatCompletionResponse = serde_json::from_value(serde_json::json!({
        "id": "chatcmpl-test",
        "choices": [{
            "message": {
                "role": "assistant",
                "content": null,
                "reasoning_content": "Let me think about which tool to call...",
                "tool_calls": [{
                    "id": "call_abc123",
                    "type": "function",
                    "function": {
                        "name": "search",
                        "arguments": "{\"query\":\"test\"}"
                    }
                }]
            },
            "finish_reason": "tool_calls"
        }],
        "usage": { "prompt_tokens": 100, "completion_tokens": 50 }
    }))
    .unwrap();

    let choice = response.choices.into_iter().next().unwrap();
    let tool_calls: Vec<ToolCall> = choice
        .message
        .tool_calls
        .unwrap_or_default()
        .into_iter()
        .map(|tc| {
            let arguments = serde_json::from_str(&tc.function.arguments)
                .unwrap_or(serde_json::Value::Object(Default::default()));
            ToolCall {
                id: tc.id,
                name: tc.function.name,
                arguments,
            }
        })
        .collect();

    let content = if tool_calls.is_empty() {
        choice.message.content.or(choice.message.reasoning_content)
    } else {
        choice.message.content
    };

    assert!(
        content.is_none(),
        "reasoning_content should NOT leak into tool-call responses, got: {:?}",
        content
    );
    assert_eq!(tool_calls.len(), 1);
    assert_eq!(tool_calls[0].name, "search");
}

/// Regression: reasoning_content SHOULD be used as fallback for text responses.
#[test]
fn test_reasoning_content_used_for_text_response() {
    let response: ChatCompletionResponse = serde_json::from_value(serde_json::json!({
        "id": "chatcmpl-test",
        "choices": [{
            "message": {
                "role": "assistant",
                "content": null,
                "reasoning_content": "The answer is 42."
            },
            "finish_reason": "stop"
        }],
        "usage": { "prompt_tokens": 50, "completion_tokens": 20 }
    }))
    .unwrap();

    let choice = response.choices.into_iter().next().unwrap();
    let tool_calls: Vec<ToolCall> = choice
        .message
        .tool_calls
        .unwrap_or_default()
        .into_iter()
        .map(|tc| {
            let arguments = serde_json::from_str(&tc.function.arguments)
                .unwrap_or(serde_json::Value::Object(Default::default()));
            ToolCall {
                id: tc.id,
                name: tc.function.name,
                arguments,
            }
        })
        .collect();

    let content = if tool_calls.is_empty() {
        choice.message.content.or(choice.message.reasoning_content)
    } else {
        choice.message.content
    };

    assert_eq!(
        content,
        Some("The answer is 42.".to_string()),
        "reasoning_content should be used as fallback for text responses"
    );
    assert!(tool_calls.is_empty());
}

// -- ChatCompletionResponse deserialization -------------------------------

#[test]
fn test_response_deserialize_basic() {
    let json = serde_json::json!({
        "id": "chatcmpl-abc123",
        "object": "chat.completion",
        "choices": [{
            "message": {
                "role": "assistant",
                "content": "Hello!"
            },
            "finish_reason": "stop"
        }],
        "usage": {
            "prompt_tokens": 10,
            "completion_tokens": 5,
            "total_tokens": 15
        }
    });
    let resp: ChatCompletionResponse = serde_json::from_value(json).unwrap();
    assert_eq!(resp.id, Some("chatcmpl-abc123".to_string()));
    assert_eq!(resp.choices.len(), 1);
    assert_eq!(resp.choices[0].message.content, Some("Hello!".to_string()));
    assert_eq!(resp.choices[0].finish_reason, Some("stop".to_string()));
    let usage = resp.usage.unwrap();
    assert_eq!(usage.prompt_tokens, Some(10));
    assert_eq!(usage.completion_tokens, Some(5));
    assert_eq!(usage.total_tokens, Some(15));
}

#[test]
fn test_response_deserialize_missing_optional_fields() {
    // Minimal response: no id, no usage, no finish_reason
    let json = serde_json::json!({
        "choices": [{
            "message": {
                "role": "assistant",
                "content": "Hi"
            },
            "finish_reason": null
        }]
    });
    let resp: ChatCompletionResponse = serde_json::from_value(json).unwrap();
    assert!(resp.id.is_none());
    assert!(resp.usage.is_none());
    assert!(resp.choices[0].finish_reason.is_none());
}

#[test]
fn test_response_deserialize_with_tool_calls() {
    let json = serde_json::json!({
        "choices": [{
            "message": {
                "role": "assistant",
                "content": null,
                "tool_calls": [
                    {
                        "id": "call_abc",
                        "type": "function",
                        "function": {
                            "name": "get_weather",
                            "arguments": "{\"city\":\"NYC\"}"
                        }
                    },
                    {
                        "id": "call_def",
                        "type": "function",
                        "function": {
                            "name": "get_time",
                            "arguments": "{}"
                        }
                    }
                ]
            },
            "finish_reason": "tool_calls"
        }]
    });
    let resp: ChatCompletionResponse = serde_json::from_value(json).unwrap();
    let tc = resp.choices[0].message.tool_calls.as_ref().unwrap();
    assert_eq!(tc.len(), 2);
    assert_eq!(tc[0].id, "call_abc");
    assert_eq!(tc[0].function.name, "get_weather");
    assert_eq!(tc[0].function.arguments, "{\"city\":\"NYC\"}");
    assert_eq!(tc[1].id, "call_def");
    assert_eq!(tc[1].function.name, "get_time");
}

#[test]
fn test_response_deserialize_ignores_unknown_fields() {
    // Real API responses have extra fields like "object", "created", "model"
    let json = serde_json::json!({
        "id": "chatcmpl-xyz",
        "object": "chat.completion",
        "created": 1700000000,
        "model": "gpt-4o",
        "system_fingerprint": "fp_abc123",
        "choices": [{
            "index": 0,
            "message": {
                "role": "assistant",
                "content": "ok"
            },
            "finish_reason": "stop",
            "logprobs": null
        }],
        "usage": {
            "prompt_tokens": 5,
            "completion_tokens": 1,
            "total_tokens": 6
        }
    });
    let resp: ChatCompletionResponse = serde_json::from_value(json).unwrap();
    assert_eq!(resp.choices[0].message.content, Some("ok".to_string()));
}

// -- parse_usage and saturate_u32 -----------------------------------------

#[test]
fn test_parse_usage_with_all_fields() {
    let usage = ChatCompletionUsage {
        prompt_tokens: Some(100),
        completion_tokens: Some(50),
        total_tokens: Some(150),
    };
    assert_eq!(parse_usage(Some(&usage)), (100, 50));
}

#[test]
fn test_parse_usage_none() {
    assert_eq!(parse_usage(None), (0, 0));
}

#[test]
fn test_parse_usage_missing_completion_falls_back_to_total_minus_prompt() {
    let usage = ChatCompletionUsage {
        prompt_tokens: Some(100),
        completion_tokens: None,
        total_tokens: Some(180),
    };
    // output = total - prompt = 80
    assert_eq!(parse_usage(Some(&usage)), (100, 80));
}

#[test]
fn test_parse_usage_missing_completion_and_prompt_uses_total() {
    let usage = ChatCompletionUsage {
        prompt_tokens: None,
        completion_tokens: None,
        total_tokens: Some(200),
    };
    // input = 0 (no prompt), output = total = 200
    assert_eq!(parse_usage(Some(&usage)), (0, 200));
}

#[test]
fn test_parse_usage_all_none() {
    let usage = ChatCompletionUsage {
        prompt_tokens: None,
        completion_tokens: None,
        total_tokens: None,
    };
    assert_eq!(parse_usage(Some(&usage)), (0, 0));
}

#[test]
fn test_saturate_u32_within_range() {
    assert_eq!(saturate_u32(0), 0);
    assert_eq!(saturate_u32(42), 42);
    assert_eq!(saturate_u32(u32::MAX as u64), u32::MAX);
}

#[test]
fn test_saturate_u32_overflow_clamps() {
    assert_eq!(saturate_u32(u32::MAX as u64 + 1), u32::MAX);
    assert_eq!(saturate_u32(u64::MAX), u32::MAX);
}

// -- ChatCompletionUsage deserialization -----------------------------------

#[test]
fn test_usage_deserialize_partial_fields() {
    // Some providers only return total_tokens
    let json = r#"{"total_tokens": 500}"#;
    let usage: ChatCompletionUsage = serde_json::from_str(json).unwrap();
    assert!(usage.prompt_tokens.is_none());
    assert!(usage.completion_tokens.is_none());
    assert_eq!(usage.total_tokens, Some(500));
}

#[test]
fn test_usage_deserialize_empty_object() {
    let json = "{}";
    let usage: ChatCompletionUsage = serde_json::from_str(json).unwrap();
    assert!(usage.prompt_tokens.is_none());
    assert!(usage.completion_tokens.is_none());
    assert!(usage.total_tokens.is_none());
}

// -- ChatCompletionToolCall serde roundtrip --------------------------------

#[test]
fn test_tool_call_serde_roundtrip() {
    let tc = ChatCompletionToolCall {
        id: "call_abc".to_string(),
        call_type: "function".to_string(),
        function: ChatCompletionToolCallFunction {
            name: "get_weather".to_string(),
            arguments: r#"{"city":"London"}"#.to_string(),
        },
    };
    let json = serde_json::to_value(&tc).unwrap();
    // "type" not "call_type" in serialized form
    assert_eq!(json["type"], "function");
    assert!(json.get("call_type").is_none());
    assert_eq!(json["id"], "call_abc");

    // Deserialize back
    let deserialized: ChatCompletionToolCall = serde_json::from_value(json).unwrap();
    assert_eq!(deserialized.id, "call_abc");
    assert_eq!(deserialized.call_type, "function");
    assert_eq!(deserialized.function.name, "get_weather");
    assert_eq!(deserialized.function.arguments, r#"{"city":"London"}"#);
}
