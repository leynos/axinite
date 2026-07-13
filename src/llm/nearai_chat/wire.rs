//! OpenAI-compatible Chat Completions wire types.
//!
//! Request/response payload structs, `ChatMessage` conversion, tool-message
//! flattening for providers that reject `role: "tool"`, and usage parsing.

use serde::{Deserialize, Serialize};

use crate::llm::provider::{ChatMessage, Role};

#[derive(Debug, Serialize)]
pub(super) struct ChatCompletionRequest {
    pub(super) model: String,
    pub(super) messages: Vec<ChatCompletionMessage>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) temperature: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) max_tokens: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) tools: Option<Vec<ChatCompletionTool>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) tool_choice: Option<String>,
}

/// Content field that serializes as either a string or an array of content parts.
///
/// - `Text("hello")` → `"content": "hello"`
/// - `Parts([...])` → `"content": [{"type": "text", ...}, {"type": "image_url", ...}]`
#[derive(Debug, Clone)]
pub(super) enum MessageContent {
    Text(String),
    Parts(Vec<crate::llm::ContentPart>),
}

impl Serialize for MessageContent {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self {
            MessageContent::Text(s) => serializer.serialize_str(s),
            MessageContent::Parts(parts) => parts.serialize(serializer),
        }
    }
}

impl<'de> Deserialize<'de> for MessageContent {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        use serde::de;
        use serde_json::Value;

        let val = Value::deserialize(deserializer)?;
        match val {
            Value::String(s) => Ok(MessageContent::Text(s)),
            Value::Array(arr) => Ok(MessageContent::Text(
                // For deserialization (responses), we only need the text content
                arr.iter()
                    .find_map(|v| {
                        if v.get("type")?.as_str()? == "text" {
                            v.get("text")?.as_str().map(String::from)
                        } else {
                            None
                        }
                    })
                    .unwrap_or_default(),
            )),
            Value::Null => Ok(MessageContent::Text(String::new())),
            _ => Err(de::Error::custom(
                "expected string, array, or null for content",
            )),
        }
    }
}

impl MessageContent {
    pub(super) fn as_text(&self) -> Option<&str> {
        match self {
            MessageContent::Text(s) if !s.is_empty() => Some(s),
            MessageContent::Text(_) => None,
            MessageContent::Parts(_) => None,
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub(super) struct ChatCompletionMessage {
    pub(super) role: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) content: Option<MessageContent>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) tool_call_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) tool_calls: Option<Vec<ChatCompletionToolCall>>,
}

/// Rewrite tool-call / tool-result messages into plain assistant/user text.
///
/// NEAR AI cloud-api does not support the OpenAI multi-turn tool-calling
/// protocol (`role: "tool"` messages). This function converts:
///   - Assistant messages with `tool_calls` → assistant text describing the calls
///   - Tool result messages (`role: "tool"`) → user messages with the result
///
/// Non-tool messages pass through unchanged.
pub(super) fn flatten_tool_messages(
    messages: Vec<ChatCompletionMessage>,
) -> Vec<ChatCompletionMessage> {
    let has_tool_msgs = messages.iter().any(|m| m.role == "tool");
    if !has_tool_msgs {
        return messages;
    }

    tracing::debug!("Flattening tool messages for NEAR AI compatibility");

    messages
        .into_iter()
        .map(|msg| {
            if let (true, Some(calls)) = (msg.role == "assistant", &msg.tool_calls) {
                // Convert assistant tool_calls into descriptive text
                let mut parts: Vec<String> = Vec::new();
                if let Some(text) = msg.content.as_ref().and_then(|c| c.as_text()) {
                    parts.push(text.to_string());
                }
                for tc in calls {
                    parts.push(format!(
                        "[Called tool `{}` with arguments: {}]",
                        tc.function.name, tc.function.arguments
                    ));
                }
                ChatCompletionMessage {
                    role: "assistant".to_string(),
                    content: Some(MessageContent::Text(parts.join("\n"))),

                    tool_call_id: None,
                    name: None,
                    tool_calls: None,
                }
            } else if msg.role == "tool" {
                // Convert tool result into a user message
                let tool_name = msg.name.as_deref().unwrap_or("unknown");
                let result = msg.content.as_ref().and_then(|c| c.as_text()).unwrap_or("");
                ChatCompletionMessage {
                    role: "user".to_string(),
                    content: Some(MessageContent::Text(format!(
                        "[Tool `{}` returned: {}]",
                        tool_name, result
                    ))),

                    tool_call_id: None,
                    name: None,
                    tool_calls: None,
                }
            } else {
                msg
            }
        })
        .collect()
}

/// Whether an assistant message carries only tool calls, so its empty
/// content should serialize as `null` rather than an empty string.
pub(super) fn is_tool_call_only_message(role: &str, has_tool_calls: bool, content: &str) -> bool {
    let assistant_with_calls = role == "assistant" && has_tool_calls;
    assistant_with_calls && content.is_empty()
}

impl From<ChatMessage> for ChatCompletionMessage {
    fn from(msg: ChatMessage) -> Self {
        let role = match msg.role {
            Role::System => "system",
            Role::User => "user",
            Role::Assistant => "assistant",
            Role::Tool => "tool",
        };

        let tool_calls = msg.tool_calls.map(|calls| {
            calls
                .into_iter()
                .map(|tc| ChatCompletionToolCall {
                    id: tc.id,
                    call_type: "function".to_string(),
                    function: ChatCompletionToolCallFunction {
                        name: tc.name,
                        arguments: tc.arguments.to_string(),
                    },
                })
                .collect()
        });

        let content = if is_tool_call_only_message(role, tool_calls.is_some(), &msg.content) {
            None
        } else if !msg.content_parts.is_empty() {
            // Build multimodal content array: text + image parts
            let mut parts = vec![crate::llm::ContentPart::Text { text: msg.content }];
            parts.extend(msg.content_parts);
            Some(MessageContent::Parts(parts))
        } else {
            Some(MessageContent::Text(msg.content))
        };

        Self {
            role: role.to_string(),
            content,
            tool_call_id: msg.tool_call_id,
            name: msg.name,
            tool_calls,
        }
    }
}

#[derive(Debug, Serialize)]
pub(super) struct ChatCompletionTool {
    #[serde(rename = "type")]
    pub(super) tool_type: String,
    pub(super) function: ChatCompletionFunction,
}

#[derive(Debug, Serialize)]
pub(super) struct ChatCompletionFunction {
    pub(super) name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) parameters: Option<serde_json::Value>,
}

#[derive(Debug, Deserialize)]
pub(super) struct ChatCompletionResponse {
    #[allow(dead_code)]
    #[serde(default)]
    pub(super) id: Option<String>,
    pub(super) choices: Vec<ChatCompletionChoice>,
    #[serde(default)]
    pub(super) usage: Option<ChatCompletionUsage>,
}

#[derive(Debug, Deserialize)]
pub(super) struct ChatCompletionChoice {
    pub(super) message: ChatCompletionResponseMessage,
    pub(super) finish_reason: Option<String>,
}

#[derive(Debug, Deserialize)]
pub(super) struct ChatCompletionResponseMessage {
    #[allow(dead_code)]
    pub(super) role: String,
    pub(super) content: Option<String>,
    /// Some models (e.g. GLM-5) return chain-of-thought reasoning here
    /// instead of in `content`.
    #[serde(default)]
    pub(super) reasoning_content: Option<String>,
    pub(super) tool_calls: Option<Vec<ChatCompletionToolCall>>,
}

#[derive(Debug, Serialize, Deserialize)]
pub(super) struct ChatCompletionToolCall {
    pub(super) id: String,
    #[serde(rename = "type")]
    #[allow(dead_code)]
    pub(super) call_type: String,
    pub(super) function: ChatCompletionToolCallFunction,
}

#[derive(Debug, Serialize, Deserialize)]
pub(super) struct ChatCompletionToolCallFunction {
    pub(super) name: String,
    pub(super) arguments: String,
}

#[derive(Debug, Deserialize, Default)]
pub(super) struct ChatCompletionUsage {
    #[serde(default)]
    pub(super) prompt_tokens: Option<u64>,
    #[serde(default)]
    pub(super) completion_tokens: Option<u64>,
    #[serde(default)]
    pub(super) total_tokens: Option<u64>,
}

pub(super) fn saturate_u32(val: u64) -> u32 {
    val.min(u32::MAX as u64) as u32
}

pub(super) fn parse_usage(usage: Option<&ChatCompletionUsage>) -> (u32, u32) {
    let Some(u) = usage else {
        return (0, 0);
    };
    let input = u.prompt_tokens.map(saturate_u32).unwrap_or(0);
    let output = u.completion_tokens.map(saturate_u32).unwrap_or_else(|| {
        // Fall back to total - prompt if completion is missing.
        match (u.total_tokens, u.prompt_tokens) {
            (Some(total), Some(prompt)) => saturate_u32(total.saturating_sub(prompt)),
            (Some(total), None) => saturate_u32(total),
            _ => 0,
        }
    });
    (input, output)
}
