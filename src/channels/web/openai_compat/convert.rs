//! Conversions between OpenAI wire types and internal LLM types, plus
//! request validation and error-mapping helpers.

use axum::{Json, http::StatusCode};

use crate::llm::{ChatMessage, FinishReason, Role, ToolCall, ToolDefinition};

use super::types::{
    OpenAiErrorDetail, OpenAiErrorResponse, OpenAiMessage, OpenAiTool, OpenAiToolCall,
    OpenAiToolCallFunction,
};

pub(super) const MAX_MODEL_NAME_BYTES: usize = 256;

pub(super) fn parse_role(s: &str) -> Result<Role, String> {
    match s {
        "system" => Ok(Role::System),
        "user" => Ok(Role::User),
        "assistant" => Ok(Role::Assistant),
        "tool" => Ok(Role::Tool),
        _ => Err(format!("Unknown role: '{}'", s)),
    }
}

pub fn convert_messages(messages: &[OpenAiMessage]) -> Result<Vec<ChatMessage>, String> {
    messages
        .iter()
        .enumerate()
        .map(|(i, m)| {
            let role = parse_role(&m.role).map_err(|e| format!("messages[{}]: {}", i, e))?;
            match role {
                Role::Tool => {
                    let tool_call_id = m.tool_call_id.as_deref().ok_or_else(|| {
                        format!("messages[{}]: tool message requires 'tool_call_id'", i)
                    })?;
                    let name = m
                        .name
                        .as_deref()
                        .ok_or_else(|| format!("messages[{}]: tool message requires 'name'", i))?;
                    Ok(ChatMessage::tool_result(
                        tool_call_id,
                        name,
                        m.content.as_deref().unwrap_or(""),
                    ))
                }
                Role::Assistant => {
                    if let Some(ref tcs) = m.tool_calls {
                        let calls: Vec<ToolCall> = tcs
                            .iter()
                            .map(|tc| ToolCall {
                                id: tc.id.clone(),
                                name: tc.function.name.clone(),
                                arguments: serde_json::from_str(&tc.function.arguments)
                                    .unwrap_or(serde_json::Value::Object(Default::default())),
                            })
                            .collect();
                        Ok(ChatMessage::assistant_with_tool_calls(
                            m.content.clone(),
                            calls,
                        ))
                    } else {
                        Ok(ChatMessage::assistant(m.content.as_deref().unwrap_or("")))
                    }
                }
                _ => Ok(ChatMessage {
                    role,
                    content: m.content.as_deref().unwrap_or("").to_string(),
                    content_parts: Vec::new(),
                    tool_call_id: None,
                    name: m.name.clone(),
                    tool_calls: None,
                }),
            }
        })
        .collect()
}

pub fn convert_tools(tools: &[OpenAiTool]) -> Vec<ToolDefinition> {
    tools
        .iter()
        .filter(|t| t.tool_type == "function")
        .map(|t| ToolDefinition {
            name: t.function.name.clone(),
            description: t.function.description.clone().unwrap_or_default(),
            parameters: t
                .function
                .parameters
                .clone()
                .unwrap_or(serde_json::json!({"type": "object", "properties": {}})),
        })
        .collect()
}

pub(super) fn convert_tool_calls_to_openai(calls: &[ToolCall]) -> Vec<OpenAiToolCall> {
    calls
        .iter()
        .map(|tc| OpenAiToolCall {
            id: tc.id.clone(),
            call_type: "function".to_string(),
            function: OpenAiToolCallFunction {
                name: tc.name.clone(),
                arguments: serde_json::to_string(&tc.arguments).unwrap_or_default(),
            },
        })
        .collect()
}

pub fn finish_reason_str(reason: FinishReason) -> String {
    match reason {
        FinishReason::Stop => "stop".to_string(),
        FinishReason::Length => "length".to_string(),
        FinishReason::ToolUse => "tool_calls".to_string(),
        FinishReason::ContentFilter => "content_filter".to_string(),
        FinishReason::Unknown => "stop".to_string(),
    }
}

pub(super) fn normalize_tool_choice(val: &serde_json::Value) -> Option<String> {
    match val {
        serde_json::Value::String(s) => Some(s.clone()),
        serde_json::Value::Object(obj) => {
            // { "type": "function", "function": { "name": "foo" } } → "required"
            if obj.contains_key("function") {
                Some("required".to_string())
            } else {
                obj.get("type")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string())
            }
        }
        _ => None,
    }
}

pub(super) fn map_llm_error(
    err: crate::error::LlmError,
) -> (StatusCode, Json<OpenAiErrorResponse>) {
    let (status, error_type, code) = match &err {
        crate::error::LlmError::AuthFailed { .. }
        | crate::error::LlmError::SessionExpired { .. } => (
            StatusCode::UNAUTHORIZED,
            "authentication_error",
            "auth_error",
        ),
        crate::error::LlmError::RateLimited { .. } => (
            StatusCode::TOO_MANY_REQUESTS,
            "rate_limit_error",
            "rate_limit",
        ),
        crate::error::LlmError::ContextLengthExceeded { .. } => (
            StatusCode::BAD_REQUEST,
            "invalid_request_error",
            "context_length_exceeded",
        ),
        crate::error::LlmError::ModelNotAvailable { .. } => (
            StatusCode::NOT_FOUND,
            "invalid_request_error",
            "model_not_found",
        ),
        _ => (
            StatusCode::INTERNAL_SERVER_ERROR,
            "server_error",
            "internal_error",
        ),
    };

    (
        status,
        Json(OpenAiErrorResponse {
            error: OpenAiErrorDetail {
                message: err.to_string(),
                error_type: error_type.to_string(),
                param: None,
                code: Some(code.to_string()),
            },
        }),
    )
}

pub(super) fn openai_error(
    status: StatusCode,
    message: impl Into<String>,
    error_type: &str,
) -> (StatusCode, Json<OpenAiErrorResponse>) {
    (
        status,
        Json(OpenAiErrorResponse {
            error: OpenAiErrorDetail {
                message: message.into(),
                error_type: error_type.to_string(),
                param: None,
                code: None,
            },
        }),
    )
}

pub(super) fn chat_completion_id() -> String {
    format!("chatcmpl-{}", uuid::Uuid::new_v4().simple())
}

pub(super) fn unix_timestamp() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

pub(super) fn validate_model_name(model: &str) -> Result<(), String> {
    let trimmed = model.trim();

    if trimmed.is_empty() {
        return Err("model must not be empty".to_string());
    }
    if trimmed != model {
        return Err("model must not have leading or trailing whitespace".to_string());
    }
    if model.len() > MAX_MODEL_NAME_BYTES {
        return Err(format!(
            "model must be at most {} bytes",
            MAX_MODEL_NAME_BYTES
        ));
    }
    if model.chars().any(char::is_control) {
        return Err("model contains control characters".to_string());
    }
    Ok(())
}

/// Extract stop sequences from the flexible `stop` field.
pub(super) fn parse_stop(val: &serde_json::Value) -> Option<Vec<String>> {
    match val {
        serde_json::Value::String(s) => Some(vec![s.clone()]),
        serde_json::Value::Array(arr) => {
            let strs: Vec<String> = arr
                .iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect();
            if strs.is_empty() { None } else { Some(strs) }
        }
        _ => None,
    }
}
