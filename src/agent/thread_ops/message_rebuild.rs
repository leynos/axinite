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
mod tests {
    use super::*;
    use crate::config::SafetyConfig;
    use crate::history::ConversationMessage;
    use crate::safety::SafetyLayer;
    use rstest::{fixture, rstest};

    fn make_db_msg(role: &str, content: &str) -> ConversationMessage {
        ConversationMessage {
            id: uuid::Uuid::new_v4(),
            role: role.to_string(),
            content: content.to_string(),
            created_at: chrono::Utc::now(),
        }
    }

    #[fixture]
    fn test_safety_layer() -> SafetyLayer {
        SafetyLayer::new(&SafetyConfig {
            injection_check_enabled: false,
            max_output_length: 100_000,
        })
    }

    /// Asserts the result contains exactly one `User` message followed by one
    /// `Assistant` message. Used to verify that a malformed or legacy
    /// `tool_calls` row is skipped entirely.
    fn assert_only_user_and_assistant(result: &[ChatMessage]) {
        assert_eq!(result.len(), 2);
        assert_eq!(result[0].role, crate::llm::Role::User);
        assert_eq!(result[1].role, crate::llm::Role::Assistant);
    }

    /// Asserts the message at `idx` has `tool_calls` set and returns a reference
    /// to the inner slice for further inspection.
    fn assert_has_tool_calls(result: &[ChatMessage], idx: usize) -> &[crate::llm::ToolCall] {
        result[idx]
            .tool_calls
            .as_deref()
            .unwrap_or_else(|| panic!("expected tool_calls to be Some on message at index {idx}"))
    }

    /// Assert that a `tool_calls` row whose JSON content is `tool_json` is
    /// skipped entirely, leaving only the surrounding user and assistant
    /// messages in the output.
    fn assert_malformed_tool_calls_skipped(safety: &SafetyLayer, tool_json: serde_json::Value) {
        let messages = vec![
            make_db_msg("user", "Hi"),
            make_db_msg("tool_calls", &tool_json.to_string()),
            make_db_msg("assistant", "Done"),
        ];
        let result = rebuild_chat_messages_from_db(&messages, safety);
        assert_only_user_and_assistant(&result);
    }

    #[rstest]
    fn test_rebuild_chat_messages_user_assistant_only(test_safety_layer: SafetyLayer) {
        let safety = test_safety_layer;
        let messages = vec![
            make_db_msg("user", "Hello"),
            make_db_msg("assistant", "Hi there!"),
        ];
        let result = rebuild_chat_messages_from_db(&messages, &safety);
        assert_only_user_and_assistant(&result);
    }

    #[rstest]
    fn test_rebuild_chat_messages_with_enriched_tool_calls(test_safety_layer: SafetyLayer) {
        let safety = test_safety_layer;
        let tool_json = serde_json::json!([
            {
                "name": "memory_search",
                "call_id": "call_0",
                "parameters": {"query": "test"},
                "result": "Found 3 results",
                "result_preview": "Found 3 re..."
            },
            {
                "name": "echo",
                "call_id": "call_1",
                "parameters": {"message": "hi"},
                "error": "timeout"
            }
        ]);
        let messages = vec![
            make_db_msg("user", "Search for test"),
            make_db_msg("tool_calls", &tool_json.to_string()),
            make_db_msg("assistant", "I found some results."),
        ];
        let result = rebuild_chat_messages_from_db(&messages, &safety);

        // user + assistant_with_tool_calls + tool_result*2 + assistant
        assert_eq!(result.len(), 5);

        // user
        assert_eq!(result[0].role, crate::llm::Role::User);

        // assistant with tool_calls
        assert_eq!(result[1].role, crate::llm::Role::Assistant);
        assert!(result[1].tool_calls.is_some());
        let tcs = assert_has_tool_calls(&result, 1);
        assert_eq!(tcs.len(), 2);
        assert_eq!(tcs[0].name, "memory_search");
        assert_eq!(tcs[0].id, "call_0");
        assert_eq!(tcs[1].name, "echo");

        // tool results - verify they contain both the original content and safety wrapper
        assert_eq!(result[2].role, crate::llm::Role::Tool);
        assert_eq!(result[2].tool_call_id, Some("call_0".to_string()));
        assert!(result[2].content.contains("Found 3 results"));
        assert!(result[2].content.contains("<tool_output"));
        assert!(result[2].content.contains("name=\"memory_search\""));

        assert_eq!(result[3].role, crate::llm::Role::Tool);
        assert_eq!(result[3].tool_call_id, Some("call_1".to_string()));
        assert!(result[3].content.contains("Error: timeout"));
        assert!(result[3].content.contains("<tool_output"));
        assert!(result[3].content.contains("name=\"echo\""));

        // final assistant
        assert_eq!(result[4].role, crate::llm::Role::Assistant);
        assert_eq!(result[4].content, "I found some results.");
    }

    #[rstest]
    fn test_rebuild_chat_messages_legacy_tool_calls_skipped(test_safety_layer: SafetyLayer) {
        // Legacy format: no call_id field
        assert_malformed_tool_calls_skipped(
            &test_safety_layer,
            serde_json::json!([
                {"name": "echo", "result_preview": "hello"}
            ]),
        );
    }

    #[rstest]
    fn test_rebuild_chat_messages_empty(test_safety_layer: SafetyLayer) {
        let safety = test_safety_layer;
        let result = rebuild_chat_messages_from_db(&[], &safety);
        assert!(result.is_empty());
    }

    #[rstest]
    fn test_rebuild_chat_messages_malformed_tool_calls_json(test_safety_layer: SafetyLayer) {
        let safety = test_safety_layer;
        let messages = vec![
            make_db_msg("user", "Hi"),
            make_db_msg("tool_calls", "not valid json"),
            make_db_msg("assistant", "Done"),
        ];
        let result = rebuild_chat_messages_from_db(&messages, &safety);
        // Malformed JSON is skipped with a warning (logs message_id and parse error)
        assert_eq!(result.len(), 2);
    }

    /// Regression tests for malformed tool_calls entries that must be skipped.
    /// Before fixes, these were silently processed with fallback values or partial data.
    #[rstest]
    #[case::missing_name(serde_json::json!([
        {"call_id": "call_0", "parameters": {"q": "x"}, "result": "ok"}
    ]))]
    #[case::mixed_valid_invalid(serde_json::json!([
        {"name": "search", "call_id": "call_0", "parameters": {}, "result": "found"},
        {"name": "write", "parameters": {"path": "a.txt"}, "result": "ok"}
    ]))]
    #[case::null_fields(serde_json::json!([
        {"name": null, "call_id": "call_0", "parameters": {}, "result": "ok"}
    ]))]
    #[case::empty_call_id(serde_json::json!([
        {"name": "search", "call_id": "", "parameters": {}, "result": "ok"}
    ]))]
    #[case::empty_name(serde_json::json!([
        {"name": "", "call_id": "call_0", "parameters": {}, "result": "ok"}
    ]))]
    #[case::whitespace_call_id(serde_json::json!([
        {"name": "search", "call_id": "   ", "parameters": {}, "result": "ok"}
    ]))]
    #[case::whitespace_name(serde_json::json!([
        {"name": "  \t  ", "call_id": "call_0", "parameters": {}, "result": "ok"}
    ]))]
    fn test_rebuild_skips_malformed_tool_calls(
        test_safety_layer: SafetyLayer,
        #[case] malformed_json: serde_json::Value,
    ) {
        assert_malformed_tool_calls_skipped(&test_safety_layer, malformed_json);
    }

    #[rstest]
    fn test_rebuild_chat_messages_multi_turn_with_tools(test_safety_layer: SafetyLayer) {
        let safety = test_safety_layer;
        let tool_json_1 = serde_json::json!([
            {"name": "search", "call_id": "call_0", "parameters": {}, "result": "found it"}
        ]);
        let tool_json_2 = serde_json::json!([
            {"name": "write", "call_id": "call_0", "parameters": {"path": "a.txt"}, "result": "ok"}
        ]);
        let messages = vec![
            make_db_msg("user", "Find X"),
            make_db_msg("tool_calls", &tool_json_1.to_string()),
            make_db_msg("assistant", "Found X"),
            make_db_msg("user", "Write it"),
            make_db_msg("tool_calls", &tool_json_2.to_string()),
            make_db_msg("assistant", "Written"),
        ];
        let result = rebuild_chat_messages_from_db(&messages, &safety);

        // Turn 1: user + assistant_with_calls + tool_result + assistant = 4
        // Turn 2: user + assistant_with_calls + tool_result + assistant = 4
        assert_eq!(result.len(), 8);

        // Verify turn boundaries
        assert_eq!(result[0].content, "Find X");
        assert!(result[1].tool_calls.is_some());
        assert_eq!(result[2].role, crate::llm::Role::Tool);
        assert_eq!(result[3].content, "Found X");

        assert_eq!(result[4].content, "Write it");
        assert!(result[5].tool_calls.is_some());
        assert_eq!(result[6].role, crate::llm::Role::Tool);
        assert_eq!(result[7].content, "Written");
    }

    #[rstest]
    fn test_tool_result_content_uses_result_preview_fallback(test_safety_layer: SafetyLayer) {
        // Entry has result_preview but no result or error — should
        // use result_preview as the content source.
        let tool_json = serde_json::json!([
            {
                "name": "search",
                "call_id": "call_preview",
                "parameters": {"q": "test"},
                "result_preview": "Preview of search results…"
            }
        ]);
        let messages = vec![
            make_db_msg("user", "Search"),
            make_db_msg("tool_calls", &tool_json.to_string()),
            make_db_msg("assistant", "Done"),
        ];
        let result = rebuild_chat_messages_from_db(&messages, &test_safety_layer);

        // user + assistant_with_tool_calls + tool_result + assistant
        assert_eq!(result.len(), 4);
        let tcs = assert_has_tool_calls(&result, 1);
        assert_eq!(tcs[0].name, "search");

        assert_eq!(result[2].role, crate::llm::Role::Tool);
        assert_eq!(result[2].tool_call_id, Some("call_preview".to_string()));
        assert!(
            result[2].content.contains("Preview of search results"),
            "tool result should contain the result_preview text"
        );
        assert!(result[2].content.contains("<tool_output"));
        assert!(result[2].content.contains("name=\"search\""));
    }

    #[rstest]
    fn test_tool_result_content_defaults_to_ok(test_safety_layer: SafetyLayer) {
        // Entry has neither error, result, nor result_preview —
        // should default to "OK".
        let tool_json = serde_json::json!([
            {
                "name": "noop",
                "call_id": "call_ok",
                "parameters": {}
            }
        ]);
        let messages = vec![
            make_db_msg("user", "Run noop"),
            make_db_msg("tool_calls", &tool_json.to_string()),
            make_db_msg("assistant", "Done"),
        ];
        let result = rebuild_chat_messages_from_db(&messages, &test_safety_layer);

        assert_eq!(result.len(), 4);
        assert_eq!(result[2].role, crate::llm::Role::Tool);
        assert_eq!(result[2].tool_call_id, Some("call_ok".to_string()));
        assert!(
            result[2].content.contains("OK"),
            "tool result should contain the default 'OK' text"
        );
        assert!(result[2].content.contains("<tool_output"));
        assert!(result[2].content.contains("name=\"noop\""));
    }
}
