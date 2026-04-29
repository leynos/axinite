//! Template substitution helpers for replayed trace tool-call arguments.

use std::borrow::Cow;
use std::collections::{HashMap, HashSet};

use ironclaw::llm::{ChatMessage, Role};

const MAX_TEMPLATE_EXPANSIONS: usize = 128;

enum TemplateExpansion {
    String(String),
    Value(serde_json::Value),
}

/// Returned when a tool-result message contains content that cannot be parsed
/// as JSON. Carries the `tool_call_id` of the offending message and the
/// underlying parse error.
#[derive(Debug)]
pub(super) struct ToolResultParseError {
    pub(super) call_id: String,
    pub(super) source: serde_json::Error,
}

impl std::fmt::Display for ToolResultParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "failed to parse tool-result content for call_id '{}': {}",
            self.call_id, self.source
        )
    }
}

impl std::error::Error for ToolResultParseError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(&self.source)
    }
}

#[inline]
fn json_scalar_to_value(value: &serde_json::Value) -> Option<serde_json::Value> {
    match value {
        serde_json::Value::String(_)
        | serde_json::Value::Number(_)
        | serde_json::Value::Bool(_)
        | serde_json::Value::Null => Some(value.clone()),
        _ => None,
    }
}

fn flatten_json_root_into_vars(
    call_id: &str,
    json: &serde_json::Value,
    vars: &mut HashMap<String, serde_json::Value>,
) {
    if let Some(obj) = json.as_object() {
        for (key, value) in obj {
            flatten_json_vars(&format!("{call_id}.{key}"), value, vars);
        }
    } else {
        flatten_json_vars(call_id, json, vars);
    }
}

/// Extracts template variables from tool-result [`ChatMessage`]s.
///
/// Iterates `messages`, skipping non-`Tool` messages and messages without a
/// `tool_call_id`. For each qualifying message the content is unwrapped from
/// an optional `<tool_output>...</tool_output>` envelope and parsed as JSON.
///
/// Object roots are flattened with `call_id.key` dot-path keys; non-object
/// roots (arrays, scalars) are keyed directly by `call_id` (arrays receive
/// indexed sub-keys, e.g. `call_id.0`).
///
/// Returns a map of dot-delimited path keys to their [`serde_json::Value`]
/// scalar leaves. Malformed JSON returns [`ToolResultParseError`] with the
/// offending call id.
///
/// # Examples
///
/// ```rust,ignore
/// # use ironclaw::llm::{ChatMessage, Role};
/// # use crate::support::trace_template_utils::extract_tool_result_vars;
/// let messages = [ChatMessage {
///     role: Role::Tool,
///     content: "<tool_output>{\"id\":7,\"items\":[\"alpha\"]}</tool_output>".to_string(),
///     content_parts: Vec::new(),
///     tool_call_id: Some("call_lookup".to_string()),
///     name: None,
///     tool_calls: None,
/// }];
///
/// let vars = extract_tool_result_vars(&messages).expect("tool output should parse");
///
/// assert_eq!(vars["call_lookup.id"], serde_json::json!(7));
/// assert_eq!(vars["call_lookup.items.0"], serde_json::json!("alpha"));
/// ```
pub(super) fn extract_tool_result_vars(
    messages: &[ChatMessage],
) -> Result<HashMap<String, serde_json::Value>, ToolResultParseError> {
    let mut vars = HashMap::new();
    for message in messages {
        if message.role != Role::Tool {
            continue;
        }
        let Some(call_id) = message.tool_call_id.as_deref() else {
            continue;
        };
        let content = unwrap_tool_output(&message.content);
        let json = serde_json::from_str::<serde_json::Value>(&content).map_err(|source| {
            ToolResultParseError {
                call_id: call_id.to_owned(),
                source,
            }
        })?;
        flatten_json_root_into_vars(call_id, &json, &mut vars);
    }
    Ok(vars)
}

fn flatten_json_vars(
    path: &str,
    value: &serde_json::Value,
    vars: &mut HashMap<String, serde_json::Value>,
) {
    if let Some(scalar_value) = json_scalar_to_value(value) {
        vars.insert(path.to_string(), scalar_value);
        return;
    }

    match value {
        serde_json::Value::Object(map) => {
            for (key, child) in map {
                flatten_json_vars(&format!("{path}.{key}"), child, vars);
            }
        }
        serde_json::Value::Array(items) => {
            for (index, child) in items.iter().enumerate() {
                flatten_json_vars(&format!("{path}.{index}"), child, vars);
            }
        }
        _ => {}
    }
}

fn unwrap_tool_output(content: &str) -> Cow<'_, str> {
    let trimmed = content.trim();
    if let Some(rest) = trimmed.strip_prefix("<tool_output")
        && let Some(tag_end) = rest.find('>')
    {
        let inner = &rest[tag_end + 1..];
        if let Some(close) = inner.rfind("</tool_output>") {
            let body = inner[..close].trim();
            return Cow::Borrowed(body);
        }
    }
    Cow::Borrowed(content)
}

fn is_exact_template(
    s: &str,
    vars: &HashMap<String, serde_json::Value>,
) -> Option<serde_json::Value> {
    if s.starts_with("{{") && s.ends_with("}}") && s.matches("{{").count() == 1 {
        let key = s[2..s.len() - 2].trim();
        return vars.get(key).cloned();
    }
    None
}

fn expand_one_template(
    result: &str,
    vars: &HashMap<String, serde_json::Value>,
) -> Option<TemplateExpansion> {
    let start = result.find("{{")?;
    let end = result[start..].find("}}").map(|end| start + end + 2)?;
    let key = result[start + 2..end - 2].trim();
    let resolved = vars.get(key)?;
    if start == 0 && end == result.len() && result.matches("{{").count() == 1 {
        return Some(match resolved.as_str() {
            Some(resolved_str) => TemplateExpansion::String(resolved_str.to_owned()),
            None => TemplateExpansion::Value(resolved.clone()),
        });
    }
    let replacement = resolved
        .as_str()
        .map(str::to_owned)
        .unwrap_or_else(|| resolved.to_string());
    let mut new_result = String::with_capacity(result.len() + replacement.len());
    new_result.push_str(&result[..start]);
    new_result.push_str(&replacement);
    new_result.push_str(&result[end..]);
    Some(TemplateExpansion::String(new_result))
}

/// Performs in-place `{{key}}` template substitution over a JSON value tree.
///
/// Recurses through objects and arrays. For each string node:
/// - If the entire string is a single `{{key}}` template and the key resolves,
///   the node is replaced with the resolved [`serde_json::Value`] (preserving
///   numeric and boolean types).
/// - Otherwise, embedded `{{key}}` placeholders are expanded iteratively up to
///   [`MAX_TEMPLATE_EXPANSIONS`] times. Expansion stops early when no `{{` is
///   found, the key is missing from `vars`, or a cycle is detected via
///   previously-visited intermediate strings.
///
/// Non-string scalar nodes (numbers, booleans, null) are left unchanged.
///
/// # Examples
///
/// ```rust,ignore
/// # use std::collections::HashMap;
/// # use crate::support::trace_template_utils::substitute_templates;
/// let vars = HashMap::from([
///     ("a".to_string(), serde_json::json!("{{limit}}")),
///     ("limit".to_string(), serde_json::json!(3)),
///     ("name".to_string(), serde_json::json!("Ada")),
/// ]);
/// let mut value = serde_json::json!({
///     "limit": "{{a}}",
///     "message": "hello {{name}}",
/// });
///
/// substitute_templates(&mut value, &vars);
///
/// assert_eq!(value["limit"], serde_json::json!(3));
/// assert_eq!(value["message"], serde_json::json!("hello Ada"));
/// ```
pub(crate) fn substitute_templates(
    value: &mut serde_json::Value,
    vars: &HashMap<String, serde_json::Value>,
) {
    match value {
        serde_json::Value::String(s) => {
            let mut result = if let Some(resolved) = is_exact_template(s, vars) {
                if let Some(resolved_str) = resolved.as_str() {
                    resolved_str.to_owned()
                } else {
                    *value = resolved;
                    return;
                }
            } else {
                s.clone()
            };

            let mut visited_results = HashSet::new();
            let mut substitutions = 0usize;
            while result.contains("{{") {
                if substitutions >= MAX_TEMPLATE_EXPANSIONS {
                    break;
                }
                if !visited_results.insert(result.clone()) {
                    break;
                }
                match expand_one_template(&result, vars) {
                    Some(TemplateExpansion::String(expanded)) => {
                        result = expanded;
                        substitutions += 1;
                    }
                    Some(TemplateExpansion::Value(resolved)) => {
                        *value = resolved;
                        return;
                    }
                    None => break,
                }
            }
            *s = result;
        }
        serde_json::Value::Object(map) => {
            for value in map.values_mut() {
                substitute_templates(value, vars);
            }
        }
        serde_json::Value::Array(array) => {
            for value in array.iter_mut() {
                substitute_templates(value, vars);
            }
        }
        _ => {}
    }
}

/// Returns whether a JSON value tree contains a template marker.
///
/// Recursively inspects the supplied [`serde_json::Value`] and returns `true`
/// when any string node contains both `{{` and `}}`.
pub(super) fn has_template_marker(value: &serde_json::Value) -> bool {
    match value {
        serde_json::Value::String(s) => s.contains("{{") && s.contains("}}"),
        serde_json::Value::Array(items) => items.iter().any(has_template_marker),
        serde_json::Value::Object(map) => map.values().any(has_template_marker),
        _ => false,
    }
}
