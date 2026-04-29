//! Template substitution helpers for replayed trace tool-call arguments.

use std::borrow::Cow;
use std::collections::{HashMap, HashSet};

use ironclaw::llm::{ChatMessage, Role};

const MAX_TEMPLATE_EXPANSIONS: usize = 128;

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
        | serde_json::Value::Bool(_) => Some(value.clone()),
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
pub(crate) fn substitute_templates(
    value: &mut serde_json::Value,
    vars: &HashMap<String, serde_json::Value>,
) {
    match value {
        serde_json::Value::String(s) => {
            if s.starts_with("{{") && s.ends_with("}}") && s.matches("{{").count() == 1 {
                let key = s[2..s.len() - 2].trim();
                if let Some(resolved) = vars.get(key) {
                    *value = resolved.clone();
                    return;
                }
            }

            let mut result = s.clone();
            let mut visited_results = HashSet::new();
            let mut substitutions = 0;
            while let Some(start) = result.find("{{") {
                if substitutions >= MAX_TEMPLATE_EXPANSIONS {
                    break;
                }
                if !visited_results.insert(result.clone()) {
                    break;
                }

                if let Some(end) = result[start..].find("}}") {
                    let end = start + end + 2;
                    let key = result[start + 2..end - 2].trim();

                    if let Some(resolved) = vars.get(key) {
                        let replacement = resolved
                            .as_str()
                            .map(str::to_owned)
                            .unwrap_or_else(|| resolved.to_string());
                        result = format!("{}{}{}", &result[..start], replacement, &result[end..]);
                        substitutions += 1;
                    } else {
                        break;
                    }
                } else {
                    break;
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
pub(super) fn value_contains_template(value: &serde_json::Value) -> bool {
    match value {
        serde_json::Value::String(s) => s.contains("{{") && s.contains("}}"),
        serde_json::Value::Array(items) => items.iter().any(value_contains_template),
        serde_json::Value::Object(map) => map.values().any(value_contains_template),
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn substitute_templates_stops_on_cyclic_references() {
        let vars = HashMap::from([
            ("first".to_string(), serde_json::json!("{{second}}")),
            ("second".to_string(), serde_json::json!("{{first}}")),
        ]);
        let mut value = serde_json::json!("path {{first}}");

        substitute_templates(&mut value, &vars);

        assert_eq!(value, serde_json::json!("path {{first}}"));
    }

    #[test]
    fn substitute_templates_allows_repeated_non_cyclic_keys() {
        let vars = HashMap::from([("name".to_string(), serde_json::json!("Ada"))]);
        let mut value = serde_json::json!("{{name}} meets {{name}}");

        substitute_templates(&mut value, &vars);

        assert_eq!(value, serde_json::json!("Ada meets Ada"));
    }

    #[test]
    fn substitute_templates_preserves_scalar_json_types() {
        let vars = HashMap::from([
            ("limit".to_string(), serde_json::json!(3)),
            ("enabled".to_string(), serde_json::json!(true)),
        ]);
        let mut value = serde_json::json!({
            "limit": "{{limit}}",
            "enabled": "{{enabled}}",
            "summary": "limit={{limit}}, enabled={{enabled}}",
        });

        substitute_templates(&mut value, &vars);

        assert_eq!(
            value,
            serde_json::json!({
                "limit": 3,
                "enabled": true,
                "summary": "limit=3, enabled=true",
            })
        );
    }

    #[test]
    fn value_contains_template_detects_nested_placeholders() {
        let value = serde_json::json!({
            "items": [
                {"plain": "value"},
                {"templated": "prefix {{call.value}} suffix"}
            ]
        });

        assert!(value_contains_template(&value));
        assert!(!value_contains_template(&serde_json::json!({
            "items": [{"plain": "value"}]
        })));
    }

    #[test]
    fn extract_tool_result_vars_flattens_non_object_roots() {
        let messages = [ChatMessage {
            role: Role::Tool,
            content: serde_json::json!(["alpha", true]).to_string(),
            content_parts: Vec::new(),
            tool_call_id: Some("call_array".to_string()),
            name: None,
            tool_calls: None,
        }];

        let vars = extract_tool_result_vars(&messages).expect("valid JSON should parse");

        assert_eq!(vars.get("call_array.0"), Some(&serde_json::json!("alpha")));
        assert_eq!(vars.get("call_array.1"), Some(&serde_json::json!(true)));
    }

    #[test]
    fn extract_tool_result_vars_errors_on_invalid_json() {
        let messages = [ChatMessage {
            role: Role::Tool,
            content: "not json {{{".to_string(),
            content_parts: Vec::new(),
            tool_call_id: Some("call_bad".to_string()),
            name: None,
            tool_calls: None,
        }];

        let result = extract_tool_result_vars(&messages);

        assert!(result.is_err());
        let err = result.expect_err("invalid JSON should return parse error");
        assert_eq!(err.call_id, "call_bad");
    }
}
