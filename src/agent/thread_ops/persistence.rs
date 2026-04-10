//! Persistence helpers for thread operations.
//!
//! Contains utilities for building database parameters and managing conversation persistence.

use std::sync::Arc;

use uuid::Uuid;

use crate::agent::Agent;
use crate::channels::web::util::truncate_preview;
use crate::db::EnsureConversationParams;

/// Context for persisting turn-related data.
///
/// Groups thread_id, user_id, and turn_number to reduce the argument count
/// of persistence functions (addresses CodeScene "Excess Number of Function Arguments").
#[derive(Clone)]
pub(crate) struct TurnPersistContext<'a> {
    pub thread_id: Uuid,
    pub user_id: &'a str,
    pub turn_number: usize,
}

/// Convert a JSON value to a preview string with the given character limit.
fn value_to_preview(v: &serde_json::Value, limit: usize) -> String {
    match v {
        serde_json::Value::String(s) => truncate_preview(s, limit),
        other => truncate_preview(&other.to_string(), limit),
    }
}

/// Summarize a single tool call into a JSON object.
fn summarize_tool_call(
    turn_number: usize,
    i: usize,
    tc: &crate::agent::session::TurnToolCall,
) -> serde_json::Value {
    let mut obj = serde_json::json!({
        "name": tc.name,
        "call_id": format!("turn{}_{}", turn_number, i),
        "parameters": serde_json::to_value(&tc.parameters)
            .unwrap_or_else(|_| serde_json::json!({})),
    });
    if let Some(ref result) = tc.result {
        obj["result_preview"] = serde_json::Value::String(value_to_preview(result, 500));
        obj["result"] = result.clone();
    }
    if let Some(ref error) = tc.error {
        obj["error"] = serde_json::Value::String(truncate_preview(error, 1000));
    }
    obj
}

/// Helper to build EnsureConversationParams for gateway conversations.
///
/// Gateway conversations use channel="gateway", id=thread_id, and thread_id=None.
pub(super) fn gateway_conversation_params(
    thread_id: Uuid,
    user_id: &str,
) -> EnsureConversationParams<'_> {
    EnsureConversationParams {
        id: thread_id,
        channel: "gateway",
        user_id,
        thread_id: None,
    }
}

impl Agent {
    /// Persist the user message to the DB at turn start (before the agentic loop).
    ///
    /// This ensures the user message is durable even if the process crashes
    /// mid-response. Call this right after `thread.start_turn()`.
    pub(super) async fn persist_user_message(
        &self,
        thread_id: Uuid,
        user_id: &str,
        user_input: &str,
    ) {
        let store = match self.store() {
            Some(s) => Arc::clone(s),
            None => return,
        };

        if let Err(e) = store
            .ensure_conversation(gateway_conversation_params(thread_id, user_id))
            .await
        {
            tracing::warn!("Failed to ensure conversation {}: {}", thread_id, e);
            return;
        }

        if let Err(e) = store
            .add_conversation_message(thread_id, "user", user_input)
            .await
        {
            tracing::warn!("Failed to persist user message: {}", e);
        }
    }

    /// Persist the assistant response to the DB after the agentic loop completes.
    ///
    /// Re-ensures the conversation row exists so that assistant responses are
    /// still persisted even if `persist_user_message` failed transiently at
    /// turn start (e.g. a brief DB blip that resolved before response time).
    pub(super) async fn persist_assistant_response(
        &self,
        thread_id: Uuid,
        user_id: &str,
        response: &str,
    ) {
        let store = match self.store() {
            Some(s) => Arc::clone(s),
            None => return,
        };

        if let Err(e) = store
            .ensure_conversation(gateway_conversation_params(thread_id, user_id))
            .await
        {
            tracing::warn!("Failed to ensure conversation {}: {}", thread_id, e);
            return;
        }

        if let Err(e) = store
            .add_conversation_message(thread_id, "assistant", response)
            .await
        {
            tracing::warn!("Failed to persist assistant message: {}", e);
        }
    }

    /// Persist tool call summaries to the DB as a `role="tool_calls"` message.
    ///
    /// Stored between the user and assistant messages so that
    /// `build_turns_from_db_messages` can reconstruct the tool call history.
    /// Content is a JSON array of tool call summaries.
    pub(super) async fn persist_tool_calls(
        &self,
        ctx: &TurnPersistContext<'_>,
        tool_calls: &[crate::agent::session::TurnToolCall],
    ) {
        if tool_calls.is_empty() {
            return;
        }

        let store = match self.store() {
            Some(s) => Arc::clone(s),
            None => return,
        };

        let summaries: Vec<serde_json::Value> = tool_calls
            .iter()
            .enumerate()
            .map(|(i, tc)| summarize_tool_call(ctx.turn_number, i, tc))
            .collect();

        let content = match serde_json::to_string(&summaries) {
            Ok(c) => c,
            Err(e) => {
                tracing::warn!("Failed to serialize tool calls: {}", e);
                return;
            }
        };

        if let Err(e) = store
            .ensure_conversation(gateway_conversation_params(ctx.thread_id, ctx.user_id))
            .await
        {
            tracing::warn!("Failed to ensure conversation {}: {}", ctx.thread_id, e);
            return;
        }

        if let Err(e) = store
            .add_conversation_message(ctx.thread_id, "tool_calls", &content)
            .await
        {
            tracing::warn!("Failed to persist tool calls: {}", e);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::session::TurnToolCall;

    #[test]
    fn summarise_tool_call_truncates_long_error() {
        let long_error: String = "X".repeat(3000);
        let tc = TurnToolCall {
            name: "shell".to_string(),
            parameters: serde_json::json!({}),
            result: None,
            error: Some(long_error.clone()),
        };
        let summary = summarise_tool_call(1, 0, &tc);
        let error_val = summary["error"].as_str().unwrap();
        assert!(
            error_val.len() <= 1003,
            "error should be truncated to ~1000 chars + '...', got {}",
            error_val.len()
        );
        assert!(error_val.ends_with("..."));
        assert!(error_val.starts_with(&long_error[..100]));
    }

    #[test]
    fn summarise_tool_call_preserves_short_error() {
        let tc = TurnToolCall {
            name: "echo".to_string(),
            parameters: serde_json::json!({}),
            result: None,
            error: Some("short error".to_string()),
        };
        let summary = summarise_tool_call(1, 0, &tc);
        assert_eq!(summary["error"].as_str().unwrap(), "short error");
    }
}
