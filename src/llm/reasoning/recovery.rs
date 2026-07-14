//! Recovery of tool calls that models emit as XML tags or bracket text in
//! the content field instead of the structured tool_calls array.

use crate::llm::{ToolCall, ToolDefinition};

/// Try to extract tool calls from content text where the model emitted them
/// as XML tags instead of using the structured tool_calls field.
///
/// Handles these formats:
/// - `<tool_call>tool_name</tool_call>` (bare name)
/// - `<tool_call>{"name":"x","arguments":{}}</tool_call>` (JSON)
/// - `<|tool_call|>...<|/tool_call|>` (pipe-delimited variant)
/// - `<function_call>...</function_call>` (function_call variant)
///
/// Only returns calls whose name matches an available tool.
pub(super) fn recover_tool_calls_from_content(
    content: &str,
    available_tools: &[ToolDefinition],
) -> Vec<ToolCall> {
    let tool_names: std::collections::HashSet<&str> =
        available_tools.iter().map(|t| t.name.as_str()).collect();
    let mut calls = Vec::new();

    for (open, close) in &[
        ("<tool_call>", "</tool_call>"),
        ("<|tool_call|>", "<|/tool_call|>"),
        ("<function_call>", "</function_call>"),
        ("<|function_call|>", "<|/function_call|>"),
    ] {
        recover_tagged_calls(content, open, close, &tool_names, &mut calls);
    }

    recover_bracket_calls(content, &tool_names, &mut calls);

    calls
}

/// Append a recovered call, assigning the next sequential `recovered_N` ID.
fn push_recovered_call(calls: &mut Vec<ToolCall>, name: &str, arguments: serde_json::Value) {
    calls.push(ToolCall {
        id: format!("recovered_{}", calls.len()),
        name: name.to_string(),
        arguments,
    });
}

/// Recover calls delimited by one `open`/`close` tag pair: JSON payloads
/// first, then bare tool names.
fn recover_tagged_calls(
    content: &str,
    open: &str,
    close: &str,
    tool_names: &std::collections::HashSet<&str>,
    calls: &mut Vec<ToolCall>,
) {
    let mut remaining = content;
    while let Some(start) = remaining.find(open) {
        let inner_start = start + open.len();
        let after = &remaining[inner_start..];
        let Some(end) = after.find(close) else {
            break;
        };
        let inner = after[..end].trim();
        remaining = &after[end + close.len()..];

        if inner.is_empty() {
            continue;
        }

        // Try JSON first: {"name":"x","arguments":{}}
        if let Some((name, arguments)) = parse_json_tool_call(inner, tool_names) {
            push_recovered_call(calls, &name, arguments);
            continue;
        }

        // Bare tool name (e.g. "<tool_call>tool_list</tool_call>")
        let name = inner.trim();
        if tool_names.contains(name) {
            push_recovered_call(calls, name, serde_json::Value::Object(Default::default()));
        }
    }
}

/// Recover calls in the bracket format emitted by `flatten_tool_messages`:
/// `[Called tool `name` with arguments: {...}]`.
fn recover_bracket_calls(
    content: &str,
    tool_names: &std::collections::HashSet<&str>,
    calls: &mut Vec<ToolCall>,
) {
    let mut remaining = content;
    while let Some(start) = remaining.find("[Called tool `") {
        let after_prefix = &remaining[start + "[Called tool `".len()..];
        let Some(backtick_end) = after_prefix.find('`') else {
            break;
        };
        let name = &after_prefix[..backtick_end];
        let after_name = &after_prefix[backtick_end + 1..];

        if !tool_names.contains(name) {
            remaining = after_name;
            continue;
        }

        // Look for " with arguments: " followed by JSON until "]"
        if let Some((arguments, rest)) = parse_bracket_arguments(after_name) {
            push_recovered_call(calls, name, arguments);
            remaining = rest;
            continue;
        }

        // No arguments or malformed — call with empty args
        push_recovered_call(calls, name, serde_json::Value::Object(Default::default()));
        remaining = after_name;
    }
}

/// Parse the ` with arguments: {...}]` suffix of a bracket call, returning
/// the arguments (empty object when the JSON is malformed) and the text
/// following the closing bracket.
fn parse_bracket_arguments(after_name: &str) -> Option<(serde_json::Value, &str)> {
    let args_start = after_name.strip_prefix(" with arguments: ")?;
    // Find the closing "]" — but the JSON itself may contain "]",
    // so find the last "]" on this logical line.
    let bracket_end = args_start.rfind(']')?;
    let args_str = &args_start[..bracket_end];
    let arguments = serde_json::from_str::<serde_json::Value>(args_str)
        .unwrap_or(serde_json::Value::Object(Default::default()));
    Some((arguments, &args_start[bracket_end + 1..]))
}

/// Parse an XML-tag payload as a JSON tool call (`{"name":..,"arguments":..}`),
/// returning its name and arguments when the name matches an available tool.
fn parse_json_tool_call(
    inner: &str,
    tool_names: &std::collections::HashSet<&str>,
) -> Option<(String, serde_json::Value)> {
    let parsed = serde_json::from_str::<serde_json::Value>(inner).ok()?;
    let name = parsed.get("name").and_then(|v| v.as_str())?;
    if !tool_names.contains(name) {
        return None;
    }
    let arguments = parsed
        .get("arguments")
        .cloned()
        .unwrap_or(serde_json::Value::Object(Default::default()));
    Some((name.to_string(), arguments))
}
