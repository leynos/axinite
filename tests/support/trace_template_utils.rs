//! Template substitution helpers for replayed trace tool-call arguments.

use std::borrow::Cow;
use std::collections::{HashMap, HashSet};

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
///   with `Role::Tool`, a `tool_call_id`, and valid JSON content contribute
///   variables.
///
/// # Returns
///
/// A map of template variable names to JSON scalar values. String, number, and
/// boolean values are preserved as `serde_json::Value` so full-template
/// substitutions can keep JSON types intact.
///
/// # Errors
///
/// This function does not return errors. Messages without a tool call id and
/// messages whose content cannot be parsed as JSON are ignored.
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
) -> HashMap<String, serde_json::Value> {
    let mut vars = HashMap::new();
    for message in messages {
        if message.role != Role::Tool {
            continue;
        }
        let Some(call_id) = message.tool_call_id.as_deref() else {
            continue;
        };
        let content = unwrap_tool_output(&message.content);
        let Ok(json) = serde_json::from_str::<serde_json::Value>(&content) else {
            continue;
        };
        flatten_json_root_into_vars(call_id, &json, &mut vars);
    }
    vars
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
    fn extract_tool_result_vars_flattens_non_object_roots() {
        let messages = [ChatMessage {
            role: Role::Tool,
            content: serde_json::json!(["alpha", true]).to_string(),
            content_parts: Vec::new(),
            tool_call_id: Some("call_array".to_string()),
            name: None,
            tool_calls: None,
        }];

        let vars = extract_tool_result_vars(&messages);

        assert_eq!(vars.get("call_array.0"), Some(&serde_json::json!("alpha")));
        assert_eq!(vars.get("call_array.1"), Some(&serde_json::json!(true)));
    }
}
