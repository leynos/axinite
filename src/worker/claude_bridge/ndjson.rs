//! NDJSON event types and payload mapping for the Claude bridge.

use serde::{Deserialize, Serialize};

use crate::worker::api::{JobEventPayload, JobEventType};

/// A Claude Code streaming event (NDJSON line from `--output-format stream-json`).
///
/// Claude Code emits one JSON object per line with these top-level types:
///
///   system    -> session init (session_id, tools, model)
///   assistant -> LLM response, nested under message.content[] as text/tool_use blocks
///   user      -> tool results, nested under message.content[] as tool_result blocks
///   result    -> final summary (is_error, duration_ms, num_turns, result text)
///
/// Content blocks live under `message.content`, NOT at the top level.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClaudeStreamEvent {
    #[serde(rename = "type")]
    pub event_type: String,

    #[serde(default)]
    pub session_id: Option<String>,

    #[serde(default)]
    pub subtype: Option<String>,

    /// For `assistant` and `user` events: the message wrapper containing content blocks.
    #[serde(default)]
    pub message: Option<MessageWrapper>,

    /// For `result` events: the final text output.
    #[serde(default)]
    pub result: Option<serde_json::Value>,

    /// For `result` events: whether the session ended in error.
    #[serde(default)]
    pub is_error: Option<bool>,

    /// For `result` events: total wall-clock duration.
    #[serde(default)]
    pub duration_ms: Option<u64>,

    /// For `result` events: number of agentic turns used.
    #[serde(default)]
    pub num_turns: Option<u32>,
}

/// Wrapper around the `message` field in assistant/user events.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MessageWrapper {
    #[serde(default)]
    pub role: Option<String>,
    #[serde(default)]
    pub content: Option<Vec<ContentBlock>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContentBlock {
    #[serde(rename = "type")]
    pub block_type: String,
    #[serde(default)]
    pub text: Option<String>,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub id: Option<String>,
    #[serde(default)]
    pub input: Option<serde_json::Value>,
    #[serde(default)]
    pub content: Option<serde_json::Value>,
    #[serde(default)]
    pub tool_use_id: Option<String>,
}

/// Convert a Claude stream event into one or more event payloads for the orchestrator.
pub(crate) fn stream_event_to_payloads(event: &ClaudeStreamEvent) -> Vec<JobEventPayload> {
    let blocks = event
        .message
        .as_ref()
        .and_then(|message| message.content.as_ref());

    match event.event_type.as_str() {
        "system" => handle_system_event(event),
        "assistant" => handle_assistant_event(blocks),
        "user" => handle_user_event(blocks),
        "result" => handle_result_event(event),
        _ => handle_fallback_event(event),
    }
}

fn handle_system_event(event: &ClaudeStreamEvent) -> Vec<JobEventPayload> {
    vec![JobEventPayload {
        event_type: JobEventType::Status,
        data: serde_json::json!({
            "message": "Claude Code session started",
            "session_id": event.session_id,
        }),
    }]
}

fn handle_assistant_event(blocks: Option<&Vec<ContentBlock>>) -> Vec<JobEventPayload> {
    let Some(blocks) = blocks else {
        return Vec::new();
    };

    let mut payloads = Vec::new();
    for block in blocks {
        match block.block_type.as_str() {
            "text" => {
                if let Some(text) = block.text.as_deref().filter(|text| !text.is_empty()) {
                    payloads.push(JobEventPayload {
                        event_type: JobEventType::Message,
                        data: serde_json::json!({
                            "role": "assistant",
                            "content": text,
                        }),
                    });
                }
            }
            "tool_use" => {
                payloads.push(JobEventPayload {
                    event_type: JobEventType::ToolUse,
                    data: serde_json::json!({
                        "tool_name": block.name,
                        "tool_use_id": block.id,
                        "input": block.input,
                    }),
                });
            }
            _ => {}
        }
    }

    payloads
}

fn handle_user_event(blocks: Option<&Vec<ContentBlock>>) -> Vec<JobEventPayload> {
    let Some(blocks) = blocks else {
        return Vec::new();
    };

    blocks
        .iter()
        .filter(|block| block.block_type == "tool_result")
        .map(|block| JobEventPayload {
            event_type: JobEventType::ToolResult,
            data: serde_json::json!({
                "tool_use_id": block.tool_use_id,
                "output": block.content,
            }),
        })
        .collect()
}

fn handle_result_event(event: &ClaudeStreamEvent) -> Vec<JobEventPayload> {
    let mut payloads = Vec::new();
    let is_error = event.is_error.unwrap_or(false);

    if let Some(text) = event
        .result
        .as_ref()
        .and_then(|value| value.as_str())
        .filter(|text| !text.is_empty())
    {
        payloads.push(JobEventPayload {
            event_type: JobEventType::Message,
            data: serde_json::json!({
                "role": "assistant",
                "content": text,
            }),
        });
    }

    payloads.push(JobEventPayload {
        event_type: JobEventType::Result,
        data: serde_json::json!({
            "status": if is_error { "error" } else { "completed" },
            "session_id": event.session_id,
            "duration_ms": event.duration_ms,
            "num_turns": event.num_turns,
        }),
    });

    payloads
}

fn handle_fallback_event(event: &ClaudeStreamEvent) -> Vec<JobEventPayload> {
    vec![JobEventPayload {
        event_type: JobEventType::Status,
        data: serde_json::json!({
            "message": format!("Claude event: {}", event.event_type),
            "raw_type": event.event_type,
        }),
    }]
}

pub(crate) fn truncate(input: &str, max_len: usize) -> &str {
    if input.len() <= max_len {
        input
    } else {
        let mut end = max_len;
        while end > 0 && !input.is_char_boundary(end) {
            end -= 1;
        }
        &input[..end]
    }
}
