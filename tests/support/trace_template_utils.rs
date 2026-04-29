//! Template substitution helpers for replayed trace tool-call arguments.

use std::borrow::Cow;
use std::collections::{HashMap, HashSet};

use anyhow::Context;
use ironclaw::llm::{ChatMessage, Role};

const MAX_TEMPLATE_EXPANSIONS: usize = 128;

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

/// Extract template variables from tool-result messages.
///
/// Flattens scalar values from JSON tool outputs into dot-separated variable
/// names keyed by tool call id. Object roots are flattened below the call id
/// (for example, `call_1.result.id`), while array and scalar roots are
/// flattened directly under the call id (for example, `call_1.0`).
///
/// # Arguments
///
/// - `messages`: Chat messages emitted so far in a trace replay. Only messages
///   with `Role::Tool` and a `tool_call_id` contribute variables.
///
/// # Returns
///
/// A map of template variable names to JSON scalar values. String, number, and
/// boolean values are preserved as `serde_json::Value` so full-template
/// substitutions can keep JSON types intact.
///
/// # Errors
///
/// Returns an error when a tool-result message has a `tool_call_id` but its
/// content cannot be parsed as JSON. The error context includes the tool call
/// id so malformed recorded output is diagnosable.
///
/// # Panics
///
/// This function does not panic.
///
/// # Examples
///
/// A tool result with call id `call_lookup` and content
/// `{"id": 3, "ok": true}` produces `call_lookup.id = 3` and
/// `call_lookup.ok = true`.
///
/// # Usage Notes
///
/// Tool output wrapped in `<tool_output>...</tool_output>` is unwrapped before
/// parsing so recorded traces can use the same helper for raw and wrapped
/// tool-result content.
pub(super) fn extract_tool_result_vars(
    messages: &[ChatMessage],
) -> anyhow::Result<HashMap<String, serde_json::Value>> {
    let mut vars = HashMap::new();
    for message in messages {
        if message.role != Role::Tool {
            continue;
        }
        let Some(call_id) = message.tool_call_id.as_deref() else {
            continue;
        };
        let content = unwrap_tool_output(&message.content);
        let json = serde_json::from_str::<serde_json::Value>(&content).with_context(|| {
            format!("failed to parse JSON tool output for tool call id `{call_id}`")
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

/// Substitute trace-template variables into a JSON value in place.
///
/// Replaces string placeholders of the form `{{variable.path}}` using values
/// produced by `extract_tool_result_vars`. When the whole string is a single
/// placeholder, the replacement keeps the original JSON scalar type. When a
/// placeholder appears inside surrounding text, the replacement is interpolated
/// as text.
///
/// # Arguments
///
/// - `value`: JSON value to mutate. Strings are checked for placeholders;
///   objects and arrays are traversed recursively.
/// - `vars`: Template variables keyed by dot-separated paths. Values should be
///   JSON scalars extracted from previous tool-result messages.
///
/// # Returns
///
/// This function returns `()`. The supplied `value` is updated in place.
///
/// # Errors
///
/// This function does not return errors. Missing variables leave the current
/// string unchanged from the first unresolved placeholder onward.
///
/// # Panics
///
/// This function does not panic.
///
/// # Examples
///
/// Given `vars["call.limit"] = 3`, the JSON string `"{{call.limit}}"` becomes
/// the JSON number `3`, while `"limit={{call.limit}}"` becomes the string
/// `"limit=3"`.
///
/// # Usage Notes
///
/// Expansion is capped by `MAX_TEMPLATE_EXPANSIONS` and also tracks previously
/// seen intermediate strings, preventing cyclic templates from looping
/// indefinitely.
pub(super) fn substitute_templates(
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

        let vars = extract_tool_result_vars(&messages)
            .expect("array-root tool output should parse into template vars");

        assert_eq!(vars.get("call_array.0"), Some(&serde_json::json!("alpha")));
        assert_eq!(vars.get("call_array.1"), Some(&serde_json::json!(true)));
    }

    #[test]
    fn extract_tool_result_vars_reports_malformed_tool_json() {
        let messages = [ChatMessage {
            role: Role::Tool,
            content: "{not json".to_string(),
            content_parts: Vec::new(),
            tool_call_id: Some("call_bad_json".to_string()),
            name: None,
            tool_calls: None,
        }];

        let err = extract_tool_result_vars(&messages)
            .expect_err("malformed tool output should fail template extraction");

        assert!(
            err.to_string().contains("call_bad_json"),
            "error should identify the malformed tool call: {err:#}"
        );
    }
}
