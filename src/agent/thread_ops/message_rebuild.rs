//! Message rebuilding logic for hydrating chat threads from database.
//!
//! Parses `role="tool_calls"` rows to reconstruct `assistant_with_tool_calls`
//! and `tool_result` messages so that the LLM sees the complete tool execution
//! history on thread hydration.

use crate::history::ConversationMessage;
use crate::llm::{ChatMessage, ToolCall};

/// A parsed tool call entry with its metadata and raw JSON.
///
/// Tuple contains: (call_id, name, arguments, raw_entry)
type ParsedCall = (String, String, serde_json::Value, serde_json::Value);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ToolResultKind<'a> {
    Error(&'a str),
    Result(&'a str),
    ResultPreview(&'a str),
    Ok,
}

/// Validates and parses a `tool_calls` JSON array.
///
/// Enforces presence and validity of both `call_id` and `name` on every
/// entry: each must be a non-null, non-empty, non-whitespace-only string.
///
/// Returns `Ok(Vec<(call_id, name, arguments, raw_entry)>)` when all entries
/// pass validation. The `raw_entry` is the original serde_json::Value for
/// each call, preserved for result extraction.
///
/// Returns `Err(Vec<usize>)` if any entry is malformed (missing, null,
/// empty, or whitespace-only `call_id` or `name`); the `Vec` contains the
/// zero-based indices of the offending entries for diagnostic logging.
/// Legacy rows without `call_id` are rejected — no silent coercion or
/// fallback is applied.
fn parse_tool_call_entries(calls: &[serde_json::Value]) -> Result<Vec<ParsedCall>, Vec<usize>> {
    let mut parsed_calls = Vec::with_capacity(calls.len());
    let mut invalid_indices = Vec::new();

    for (idx, call) in calls.iter().enumerate() {
        // Extract and validate call_id
        let call_id = call
            .get("call_id")
            .and_then(|v| v.as_str())
            .filter(|s| !s.trim().is_empty());

        // Extract and validate name
        let name = call
            .get("name")
            .and_then(|v| v.as_str())
            .filter(|s| !s.trim().is_empty());

        match (call_id, name) {
            (Some(call_id_str), Some(name_str)) => {
                let arguments = call
                    .get("parameters")
                    .cloned()
                    .unwrap_or(serde_json::json!({}));
                let raw_entry = call.clone();
                parsed_calls.push((
                    call_id_str.to_string(),
                    name_str.to_string(),
                    arguments,
                    raw_entry,
                ));
            }
            _ => {
                invalid_indices.push(idx);
            }
        }
    }

    if !invalid_indices.is_empty() {
        return Err(invalid_indices);
    }

    Ok(parsed_calls)
}

/// Parse JSON array from enriched tool_calls row content.
///
/// Returns `None` if parsing fails or the array is empty.
fn parse_calls_json(message_id: uuid::Uuid, content: &str) -> Option<Vec<serde_json::Value>> {
    let calls = match serde_json::from_str::<Vec<serde_json::Value>>(content) {
        Ok(calls) => calls,
        Err(error) => {
            tracing::warn!(
                message_id = %message_id,
                error = %error,
                "Skipping tool_calls row with invalid JSON"
            );
            return None;
        }
    };

    if calls.is_empty() {
        tracing::trace!(message_id = %message_id, "Skipping empty tool_calls row");
        return None;
    }

    Some(calls)
}

/// Build a `ToolCall` list from validated call entries.
fn build_tool_calls(parsed_calls: &[ParsedCall]) -> Vec<ToolCall> {
    parsed_calls
        .iter()
        .map(|(id, name, arguments, _)| ToolCall {
            id: id.clone(),
            name: name.clone(),
            arguments: arguments.clone(),
        })
        .collect()
}

/// Classifies the result content from a tool call entry.
///
/// Checks fields in precedence order: "error" first, then "result",
/// then "result_preview". Returns [`ToolResultKind::Ok`] if none matched.
fn classify_result_content(entry: &serde_json::Value) -> ToolResultKind<'_> {
    if let Some(error) = entry.get("error").and_then(|value| value.as_str()) {
        return ToolResultKind::Error(error);
    }

    if let Some(result) = entry.get("result").and_then(|value| value.as_str()) {
        return ToolResultKind::Result(result);
    }

    if let Some(preview) = entry.get("result_preview").and_then(|value| value.as_str()) {
        return ToolResultKind::ResultPreview(preview);
    }

    ToolResultKind::Ok
}

/// Extracts the result-content string for a single tool call entry.
///
/// Prefers `error` (formatted as `"Error: …"`), then `result`, then
/// `result_preview`, defaulting to `"OK"`.
///
/// Applies `SafetyLayer` sanitization and wrapping to the raw content
/// before returning it (sanitizer → validator → policy → leak-detector).
fn tool_result_content(
    entry: &serde_json::Value,
    tool_name: &str,
    safety: &crate::safety::SafetyLayer,
) -> String {
    let raw_content = match classify_result_content(entry) {
        ToolResultKind::Error(error) => format!("Error: {error}"),
        ToolResultKind::Result(result) | ToolResultKind::ResultPreview(result) => {
            result.to_string()
        }
        ToolResultKind::Ok => "OK".to_string(),
    };

    let sanitized = safety.sanitize_tool_output(tool_name, &raw_content);
    safety.wrap_for_llm(tool_name, &sanitized.content, sanitized.was_modified)
}

/// Process a `tool_calls` row and append reconstructed messages.
///
/// Skips legacy rows (without call_id), malformed rows, and malformed JSON.
fn handle_tool_calls_row(
    out: &mut Vec<ChatMessage>,
    message: &ConversationMessage,
    safety: &crate::safety::SafetyLayer,
) {
    let Some(calls) = parse_calls_json(message.id, &message.content) else {
        return;
    };

    let parsed_calls = match parse_tool_call_entries(&calls) {
        Ok(parsed_calls) => parsed_calls,
        Err(invalid_indices) => {
            tracing::warn!(
                message_id = %message.id,
                total_calls = calls.len(),
                invalid_indices = ?invalid_indices,
                "Skipping malformed tool_calls row: missing call_id or name in at least one entry"
            );
            return;
        }
    };

    out.push(ChatMessage::assistant_with_tool_calls(
        None,
        build_tool_calls(&parsed_calls),
    ));

    for (call_id, name, _, entry) in &parsed_calls {
        out.push(ChatMessage::tool_result(
            call_id.clone(),
            name.clone(),
            tool_result_content(entry, name, safety),
        ));
    }
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
            "tool_calls" => handle_tool_calls_row(&mut result, msg, safety),
            _ => {} // Skip unknown roles
        }
    }

    result
}

#[cfg(test)]
mod unit_tests {
    use super::*;

    /// Unit tests for classify_result_content to ensure precedence ordering:
    /// error > result > result_preview > Ok
    #[test]
    fn test_classify_result_content_error_precedence_over_result() {
        // When both "error" and "result" are present, Error should be returned
        let entry = serde_json::json!({
            "error": "timeout",
            "result": "some data"
        });
        let kind = classify_result_content(&entry);
        assert!(matches!(kind, ToolResultKind::Error("timeout")));
    }

    #[test]
    fn test_classify_result_content_result_precedence_over_preview() {
        // When both "result" and "result_preview" are present, Result should be returned
        let entry = serde_json::json!({
            "result": "full result data",
            "result_preview": "preview..."
        });
        let kind = classify_result_content(&entry);
        assert!(matches!(kind, ToolResultKind::Result("full result data")));
    }

    #[test]
    fn test_classify_result_content_preview_fallback() {
        // When only "result_preview" is present, ResultPreview should be returned
        let entry = serde_json::json!({
            "result_preview": "preview data"
        });
        let kind = classify_result_content(&entry);
        assert!(matches!(
            kind,
            ToolResultKind::ResultPreview("preview data")
        ));
    }

    #[test]
    fn test_classify_result_content_ok_when_empty() {
        // When no result fields are present, Ok should be returned
        let entry = serde_json::json!({});
        let kind = classify_result_content(&entry);
        assert!(matches!(kind, ToolResultKind::Ok));
    }
}

#[cfg(test)]
#[path = "message_rebuild_tests.rs"]
mod tests;
