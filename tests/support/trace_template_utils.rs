//! Template substitution helpers for replayed trace tool-call arguments.

use std::borrow::Cow;
use std::collections::{HashMap, HashSet};

use ironclaw::llm::{ChatMessage, Role};

#[inline]
fn json_scalar_to_string(value: &serde_json::Value) -> Option<String> {
    match value {
        serde_json::Value::String(s) => Some(s.clone()),
        serde_json::Value::Number(n) => Some(n.to_string()),
        serde_json::Value::Bool(b) => Some(b.to_string()),
        _ => None,
    }
}

pub(super) fn extract_tool_result_vars(messages: &[ChatMessage]) -> HashMap<String, String> {
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
        let Some(obj) = json.as_object() else {
            continue;
        };
        for (key, value) in obj {
            flatten_json_vars(&format!("{call_id}.{key}"), value, &mut vars);
        }
    }
    vars
}

fn flatten_json_vars(path: &str, value: &serde_json::Value, vars: &mut HashMap<String, String>) {
    if let Some(string_value) = json_scalar_to_string(value) {
        vars.insert(path.to_string(), string_value);
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

pub(super) fn substitute_templates(value: &mut serde_json::Value, vars: &HashMap<String, String>) {
    match value {
        serde_json::Value::String(s) => {
            if s.starts_with("{{") && s.ends_with("}}") && s.matches("{{").count() == 1 {
                let key = s[2..s.len() - 2].trim();
                if let Some(resolved) = vars.get(key) {
                    *s = resolved.clone();
                    return;
                }
            }

            let mut result = s.clone();
            let mut visited_keys = HashSet::new();
            while let Some(start) = result.find("{{") {
                if let Some(end) = result[start..].find("}}") {
                    let end = start + end + 2;
                    let key = result[start + 2..end - 2].trim().to_string();

                    if !visited_keys.insert(key.clone()) {
                        break;
                    }

                    if let Some(resolved) = vars.get(&key) {
                        result = format!("{}{}{}", &result[..start], resolved, &result[end..]);
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
            ("first".to_string(), "{{second}}".to_string()),
            ("second".to_string(), "{{first}}".to_string()),
        ]);
        let mut value = serde_json::json!("path {{first}}");

        substitute_templates(&mut value, &vars);

        assert_eq!(value, serde_json::json!("path {{first}}"));
    }
}
