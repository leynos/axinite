//! Message rebuilding logic for hydrating chat threads from database.
//!
//! Parses `role="tool_calls"` rows to reconstruct `assistant_with_tool_calls`
//! and `tool_result` messages so that the LLM sees the complete tool execution
//! history on thread hydration.

use crate::history::ConversationMessage;
use crate::llm::{ChatMessage, ToolCall};

/// A parsed tool call entry with its metadata and raw JSON.
#[derive(Debug, Clone)]
struct ParsedCall {
    /// The unique identifier for this tool call.
    call_id: String,
    /// The name of the tool being called.
    name: String,
    /// The arguments/parameters passed to the tool.
    arguments: serde_json::Value,
    /// The original raw JSON entry from the database.
    raw_entry: serde_json::Value,
}

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
/// Returns `Ok(Vec<ParsedCall>)` when all entries pass validation. The
/// `raw_entry` field is the original serde_json::Value for each call,
/// preserved for result extraction.
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
                parsed_calls.push(ParsedCall {
                    call_id: call_id_str.to_string(),
                    name: name_str.to_string(),
                    arguments,
                    raw_entry,
                });
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
        .map(|pc| ToolCall {
            id: pc.call_id.clone(),
            name: pc.name.clone(),
            arguments: pc.arguments.clone(),
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
/// Passes the raw content through `SafetyLayer::sanitize_tool_output`
/// (leak detection → policy enforcement → sanitiser) and
/// `SafetyLayer::wrap_for_llm` (XML boundary wrapping) before returning.
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
            // Check if all entries are legacy format (no valid call_id in any entry).
            // Legacy rows should be skipped silently without warning.
            let all_legacy = calls.iter().all(|call| {
                call.get("call_id")
                    .and_then(|v| v.as_str())
                    .filter(|s| !s.trim().is_empty())
                    .is_none()
            });

            if all_legacy {
                return;
            }

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

    for pc in &parsed_calls {
        out.push(ChatMessage::tool_result(
            pc.call_id.clone(),
            pc.name.clone(),
            tool_result_content(&pc.raw_entry, &pc.name, safety),
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
    use rstest::rstest;

    /// Parameterised test for `classify_result_content` precedence ordering:
    /// error > result > result_preview > Ok.
    #[rstest]
    #[case::error_over_result(
        serde_json::json!({"error": "timeout", "result": "some data"}),
        "Error"
    )]
    #[case::result_over_preview(
        serde_json::json!({"result": "full result data", "result_preview": "preview..."}),
        "Result"
    )]
    #[case::preview_fallback(
        serde_json::json!({"result_preview": "preview data"}),
        "ResultPreview"
    )]
    #[case::ok_when_empty(
        serde_json::json!({}),
        "Ok"
    )]
    fn test_classify_result_content(
        #[case] entry: serde_json::Value,
        #[case] expected_variant: &str,
    ) {
        let kind = classify_result_content(&entry);
        match expected_variant {
            "Error" => assert!(matches!(kind, ToolResultKind::Error(_))),
            "Result" => assert!(matches!(kind, ToolResultKind::Result(_))),
            "ResultPreview" => assert!(matches!(kind, ToolResultKind::ResultPreview(_))),
            "Ok" => assert!(matches!(kind, ToolResultKind::Ok)),
            _ => panic!("unexpected variant {expected_variant}"),
        }
    }
}

#[cfg(test)]
#[path = "message_rebuild_tests.rs"]
mod tests;
