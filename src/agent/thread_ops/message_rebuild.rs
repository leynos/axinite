//! Message rebuilding logic for hydrating chat threads from database.
//!
//! Parses `role="tool_calls"` rows to reconstruct `assistant_with_tool_calls`
//! and `tool_result` messages so that the LLM sees the complete tool execution
//! history on thread hydration.

use crate::history::ConversationMessage;
use crate::llm::{ChatMessage, ToolCall};

/// Validates and parses a `tool_calls` JSON array.
///
/// Enforces presence and validity of both `call_id` and `name` on every
/// entry: each must be a non-null, non-empty, non-whitespace-only string.
///
/// Returns `Ok(Vec<(call_id, name, arguments)>)` when all entries pass
/// validation.
///
/// Returns `Err(Vec<usize>)` if any entry is malformed (missing, null,
/// empty, or whitespace-only `call_id` or `name`); the `Vec` contains the
/// zero-based indices of the offending entries for diagnostic logging.
/// Legacy rows without `call_id` are rejected — no silent coercion or
/// fallback is applied.
fn parse_tool_call_entries(
    calls: &[serde_json::Value],
) -> Result<Vec<(String, String, serde_json::Value)>, Vec<usize>> {
    let invalid_indices: Vec<usize> = calls
        .iter()
        .enumerate()
        .filter(|(_, c)| {
            // Reject missing, null, or empty/whitespace-only strings
            let call_id_invalid = c
                .get("call_id")
                .and_then(|v| v.as_str())
                .map(|s| s.trim().is_empty())
                .unwrap_or(true);
            let name_invalid = c
                .get("name")
                .and_then(|v| v.as_str())
                .map(|s| s.trim().is_empty())
                .unwrap_or(true);
            call_id_invalid || name_invalid
        })
        .map(|(idx, _)| idx)
        .collect();

    if !invalid_indices.is_empty() {
        return Err(invalid_indices);
    }

    Ok(calls
        .iter()
        .filter_map(|c| {
            let call_id = c.get("call_id")?.as_str()?.to_string();
            let name = c.get("name")?.as_str()?.to_string();
            let arguments = c
                .get("parameters")
                .cloned()
                .unwrap_or(serde_json::json!({}));
            Some((call_id, name, arguments))
        })
        .collect())
}

/// Extracts the result-content string for a single tool call entry.
///
/// Prefers `error` (formatted as `"Error: …"`), then `result`, then
/// `result_preview`, defaulting to `"OK"`.
///
/// Applies `SafetyLayer` sanitization and wrapping to the raw content
/// before returning it (sanitizer → validator → policy → leak-detector).
fn tool_result_content(
    call: &serde_json::Value,
    tool_name: &str,
    safety: &crate::safety::SafetyLayer,
) -> String {
    let raw_content = if let Some(err) = call.get("error").and_then(|v| v.as_str()) {
        format!("Error: {}", err)
    } else if let Some(res) = call.get("result").and_then(|v| v.as_str()) {
        res.to_string()
    } else if let Some(preview) = call.get("result_preview").and_then(|v| v.as_str()) {
        preview.to_string()
    } else {
        "OK".to_string()
    };

    let sanitized = safety.sanitize_tool_output(tool_name, &raw_content);
    safety.wrap_for_llm(tool_name, &sanitized.content, sanitized.was_modified)
}

/// Rebuild full LLM-compatible `ChatMessage` sequence from DB messages.
///
/// Parses `role="tool_calls"` rows to reconstruct
/// `assistant_with_tool_calls` and `tool_result` messages so that the LLM
/// sees the complete tool execution history on thread hydration.
///
/// Each tool-call entry must contain valid, non-empty `call_id` and `name`
/// strings. Rows with any malformed entries (missing, null, empty, or
/// whitespace-only fields) are skipped entirely — legacy rows without
/// `call_id` are no longer accepted or silently coerced.
///
/// Hydrated tool results pass through `SafetyLayer` (sanitizer → validator
/// → policy → leak-detector) before being added to the message sequence.
pub(super) fn rebuild_chat_messages_from_db(
    db_messages: &[ConversationMessage],
    safety: &crate::safety::SafetyLayer,
) -> Vec<ChatMessage> {
    let mut result = Vec::new();

    for msg in db_messages {
        match msg.role.as_str() {
            "user" => result.push(ChatMessage::user(&msg.content)),
            "assistant" => result.push(ChatMessage::assistant(&msg.content)),
            "tool_calls" => {
                let calls = match serde_json::from_str::<Vec<serde_json::Value>>(&msg.content) {
                    Ok(calls) => calls,
                    Err(e) => {
                        tracing::warn!(
                            message_id = %msg.id,
                            error = %e,
                            "Skipping tool_calls row with invalid JSON"
                        );
                        continue;
                    }
                };

                if calls.is_empty() {
                    continue;
                }

                match parse_tool_call_entries(&calls) {
                    Err(invalid_indices) => {
                        tracing::warn!(
                            message_id = %msg.id,
                            total_calls = calls.len(),
                            invalid_indices = ?invalid_indices,
                            "Skipping malformed tool_calls row: missing call_id or name in at least one entry"
                        );
                        continue;
                    }
                    Ok(parsed_calls) => {
                        let tool_calls: Vec<ToolCall> = parsed_calls
                            .iter()
                            .map(|(id, name, arguments)| ToolCall {
                                id: id.clone(),
                                name: name.clone(),
                                arguments: arguments.clone(),
                            })
                            .collect();
                        result.push(ChatMessage::assistant_with_tool_calls(None, tool_calls));

                        for (idx, (call_id, name, _)) in parsed_calls.iter().enumerate() {
                            result.push(ChatMessage::tool_result(
                                call_id.clone(),
                                name.clone(),
                                tool_result_content(&calls[idx], name, safety),
                            ));
                        }
                    }
                }
            }
            _ => {} // Skip unknown roles
        }
    }

    result
}

#[cfg(test)]
#[path = "message_rebuild_tests.rs"]
mod tests;
