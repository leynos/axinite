//! Shared conversion and normalisation helpers for the rig adapter.
//!
//! These helpers keep provider-facing request and response shaping consistent
//! across the adapter implementation and its tests.

use super::*;

/// Normalise an optional raw tool-call ID into a non-empty identifier.
///
/// Returns the trimmed `raw` value when it is present and non-empty. Otherwise,
/// generates a deterministic fallback string using `seed`.
pub(super) fn normalized_tool_call_id(raw: Option<&str>, seed: usize) -> String {
    match raw.map(str::trim).filter(|id| !id.is_empty()) {
        Some(id) => id.to_string(),
        None => format!("generated_tool_call_{seed}"),
    }
}

/// Convert IronClaw tool definitions to rig-core format.
///
/// Applies OpenAI strict-mode schema normalisation to ensure all tool
/// parameter schemas comply with OpenAI's function calling requirements.
pub(super) fn convert_tools(tools: &[IronToolDefinition]) -> Vec<RigToolDefinition> {
    tools
        .iter()
        .map(|t| RigToolDefinition {
            name: t.name.clone(),
            description: t.description.clone(),
            parameters: normalize_schema_strict(&t.parameters),
        })
        .collect()
}

/// Convert IronClaw tool_choice string to rig-core ToolChoice.
pub(super) fn convert_tool_choice(choice: Option<&str>) -> Option<RigToolChoice> {
    match choice.map(|s| s.to_lowercase()).as_deref() {
        Some("auto") => Some(RigToolChoice::Auto),
        Some("required") => Some(RigToolChoice::Required),
        Some("none") => Some(RigToolChoice::None),
        _ => None,
    }
}

/// Extract text and tool calls from a rig-core completion response.
pub(super) fn extract_response(
    choice: &OneOrMany<AssistantContent>,
    _usage: &RigUsage,
) -> (Option<String>, Vec<IronToolCall>, FinishReason) {
    let mut text_parts: Vec<String> = Vec::new();
    let mut tool_calls: Vec<IronToolCall> = Vec::new();

    for content in choice.iter() {
        match content {
            AssistantContent::Text(t) if !t.text.is_empty() => {
                text_parts.push(t.text.clone());
            }
            AssistantContent::Text(_) => {}
            AssistantContent::ToolCall(tc) => {
                tool_calls.push(IronToolCall {
                    id: tc.id.clone(),
                    name: tc.function.name.clone(),
                    arguments: tc.function.arguments.clone(),
                });
            }
            // Reasoning and Image variants are not mapped to IronClaw types
            _ => {}
        }
    }

    let text = if text_parts.is_empty() {
        None
    } else {
        Some(text_parts.join(""))
    };

    let finish = if !tool_calls.is_empty() {
        FinishReason::ToolUse
    } else {
        FinishReason::Stop
    };

    (text, tool_calls, finish)
}

/// Saturate u64 to u32 for token counts.
pub(super) fn saturate_u32(val: u64) -> u32 {
    val.min(u32::MAX as u64) as u32
}

/// Returns `true` if the model supports Anthropic prompt caching.
///
/// Per Anthropic docs, only Claude 3+ models support prompt caching.
/// Unsupported: claude-2, claude-2.1, claude-instant-*.
pub(super) fn supports_prompt_cache(name: &str) -> bool {
    let lower = name.to_lowercase();
    // Strip optional provider prefix (e.g. "anthropic/claude-...")
    let model = lower.strip_prefix("anthropic/").unwrap_or(&lower);
    // Only Claude 3+ families support prompt caching
    model.starts_with("claude-3")
        || model.starts_with("claude-4")
        || model.starts_with("claude-sonnet")
        || model.starts_with("claude-opus")
        || model.starts_with("claude-haiku")
}

/// Extract `cache_creation_input_tokens` from the raw provider response.
///
/// Rig-core's unified `Usage` does not surface this field, but Anthropic's raw
/// response includes it at `usage.cache_creation_input_tokens`. We serialize the
/// raw response to JSON and attempt to read the value.
pub(super) fn extract_cache_creation<T: Serialize>(raw: &T) -> u32 {
    serde_json::to_value(raw)
        .ok()
        .and_then(|v| v.get("usage")?.get("cache_creation_input_tokens")?.as_u64())
        .map(|n| n.min(u32::MAX as u64) as u32)
        .unwrap_or(0)
}

/// Normalise a tool call name returned by an OpenAI-compatible provider.
///
/// Some proxies (e.g. VibeProxy) prepend `proxy_` to tool names.
/// If the returned name doesn't match any known tool but stripping a
/// `proxy_` prefix yields a match, use the stripped version.
pub(super) fn normalize_tool_name(name: &str, known_tools: &HashSet<String>) -> String {
    if known_tools.contains(name) {
        return name.to_string();
    }

    if let Some(stripped) = name.strip_prefix("proxy_")
        && known_tools.contains(stripped)
    {
        return stripped.to_string();
    }

    name.to_string()
}
