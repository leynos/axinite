//! Message rebuilding logic for hydrating chat threads from database.
//!
//! Parses `role="tool_calls"` rows to reconstruct `assistant_with_tool_calls`
//! and `tool_result` messages so that the LLM sees the complete tool execution
//! history on thread hydration.

use crate::history::ConversationMessage;
use crate::llm::{ChatMessage, ToolCall};

/// Rebuild full LLM-compatible `ChatMessage` sequence from DB messages.
///
/// Parses `role="tool_calls"` rows to reconstruct `assistant_with_tool_calls`
/// and `tool_result` messages so that the LLM sees the complete tool execution
/// history on thread hydration. Falls back gracefully for legacy rows that
/// lack the enriched fields (`call_id`, `parameters`, `result`).
pub(super) fn rebuild_chat_messages_from_db(
    db_messages: &[ConversationMessage],
) -> Vec<ChatMessage> {
    let mut result = Vec::new();

    for msg in db_messages {
        match msg.role.as_str() {
            "user" => result.push(ChatMessage::user(&msg.content)),
            "assistant" => result.push(ChatMessage::assistant(&msg.content)),
            "tool_calls" => {
                // Try to parse the enriched JSON and rebuild tool messages.
                if let Ok(calls) = serde_json::from_str::<Vec<serde_json::Value>>(&msg.content) {
                    if calls.is_empty() {
                        continue;
                    }

                    // Check if this is an enriched row (has call_id) or legacy
                    let has_call_id = calls
                        .first()
                        .and_then(|c| c.get("call_id"))
                        .and_then(|v| v.as_str())
                        .is_some();

                    if has_call_id {
                        // Build assistant_with_tool_calls + tool_result messages
                        let tool_calls: Vec<ToolCall> = calls
                            .iter()
                            .map(|c| ToolCall {
                                id: c["call_id"].as_str().unwrap_or("call_0").to_string(),
                                name: c["name"].as_str().unwrap_or("unknown").to_string(),
                                arguments: c
                                    .get("parameters")
                                    .cloned()
                                    .unwrap_or(serde_json::json!({})),
                            })
                            .collect();

                        // The assistant text for tool_calls is always None here;
                        // the final assistant response comes as a separate
                        // "assistant" row after this tool_calls row.
                        result.push(ChatMessage::assistant_with_tool_calls(None, tool_calls));

                        // Emit tool_result messages for each call
                        for c in &calls {
                            let call_id = c["call_id"].as_str().unwrap_or("call_0").to_string();
                            let name = c["name"].as_str().unwrap_or("unknown").to_string();
                            let content = if let Some(err) = c.get("error").and_then(|v| v.as_str())
                            {
                                format!("Error: {}", err)
                            } else if let Some(res) = c.get("result").and_then(|v| v.as_str()) {
                                res.to_string()
                            } else if let Some(preview) =
                                c.get("result_preview").and_then(|v| v.as_str())
                            {
                                preview.to_string()
                            } else {
                                "OK".to_string()
                            };
                            result.push(ChatMessage::tool_result(call_id, name, content));
                        }
                    }
                    // Legacy rows without call_id: skip (will appear as
                    // simple user/assistant pairs, same as before this fix).
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
    use crate::history::ConversationMessage;

    fn make_db_msg(role: &str, content: &str) -> ConversationMessage {
        ConversationMessage {
            id: uuid::Uuid::new_v4(),
            role: role.to_string(),
            content: content.to_string(),
            created_at: chrono::Utc::now(),
        }
    }

    #[test]
    fn test_rebuild_chat_messages_user_assistant_only() {
        let messages = vec![
            make_db_msg("user", "Hello"),
            make_db_msg("assistant", "Hi there!"),
        ];
        let result = rebuild_chat_messages_from_db(&messages);
        assert_eq!(result.len(), 2);
        assert_eq!(result[0].role, crate::llm::Role::User);
        assert_eq!(result[1].role, crate::llm::Role::Assistant);
    }

    #[test]
    fn test_rebuild_chat_messages_with_enriched_tool_calls() {
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
        let result = rebuild_chat_messages_from_db(&messages);

        // user + assistant_with_tool_calls + tool_result*2 + assistant
        assert_eq!(result.len(), 5);

        // user
        assert_eq!(result[0].role, crate::llm::Role::User);

        // assistant with tool_calls
        assert_eq!(result[1].role, crate::llm::Role::Assistant);
        assert!(result[1].tool_calls.is_some());
        let tcs = result[1].tool_calls.as_ref().unwrap();
        assert_eq!(tcs.len(), 2);
        assert_eq!(tcs[0].name, "memory_search");
        assert_eq!(tcs[0].id, "call_0");
        assert_eq!(tcs[1].name, "echo");

        // tool results
        assert_eq!(result[2].role, crate::llm::Role::Tool);
        assert_eq!(result[2].tool_call_id, Some("call_0".to_string()));
        assert!(result[2].content.contains("Found 3 results"));

        assert_eq!(result[3].role, crate::llm::Role::Tool);
        assert_eq!(result[3].tool_call_id, Some("call_1".to_string()));
        assert!(result[3].content.contains("Error: timeout"));

        // final assistant
        assert_eq!(result[4].role, crate::llm::Role::Assistant);
        assert_eq!(result[4].content, "I found some results.");
    }

    #[test]
    fn test_rebuild_chat_messages_legacy_tool_calls_skipped() {
        // Legacy format: no call_id field
        let tool_json = serde_json::json!([
            {"name": "echo", "result_preview": "hello"}
        ]);
        let messages = vec![
            make_db_msg("user", "Hi"),
            make_db_msg("tool_calls", &tool_json.to_string()),
            make_db_msg("assistant", "Done"),
        ];
        let result = rebuild_chat_messages_from_db(&messages);

        // Legacy rows are skipped, only user + assistant
        assert_eq!(result.len(), 2);
        assert_eq!(result[0].role, crate::llm::Role::User);
        assert_eq!(result[1].role, crate::llm::Role::Assistant);
    }

    #[test]
    fn test_rebuild_chat_messages_empty() {
        let result = rebuild_chat_messages_from_db(&[]);
        assert!(result.is_empty());
    }

    #[test]
    fn test_rebuild_chat_messages_malformed_tool_calls_json() {
        let messages = vec![
            make_db_msg("user", "Hi"),
            make_db_msg("tool_calls", "not valid json"),
            make_db_msg("assistant", "Done"),
        ];
        let result = rebuild_chat_messages_from_db(&messages);
        // Malformed JSON is silently skipped
        assert_eq!(result.len(), 2);
    }

    #[test]
    fn test_rebuild_chat_messages_multi_turn_with_tools() {
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
        let result = rebuild_chat_messages_from_db(&messages);

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
}
