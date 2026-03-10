use std::collections::HashSet;

use serde_json::{Map, Value as JsonValue};

pub(super) fn normalize_schema_recursive(schema: &mut JsonValue) {
    let obj = match schema.as_object_mut() {
        Some(o) => o,
        None => return,
    };

    for key in &["anyOf", "oneOf", "allOf"] {
        if let Some(JsonValue::Array(variants)) = obj.get_mut(*key) {
            for variant in variants.iter_mut() {
                normalize_schema_recursive(variant);
            }
        }
    }

    if let Some(items) = obj.get_mut("items") {
        normalize_schema_recursive(items);
    }

    for key in &["not", "if", "then", "else"] {
        if let Some(sub) = obj.get_mut(*key) {
            normalize_schema_recursive(sub);
        }
    }

    if let Some(additional) = obj.get_mut("additionalProperties")
        && additional.is_object()
    {
        normalize_schema_recursive(additional);
    }

    let is_object = obj
        .get("type")
        .and_then(|t| t.as_str())
        .map(|t| t == "object")
        .unwrap_or(false);
    let has_properties = obj.contains_key("properties");

    if !is_object && !has_properties {
        return;
    }

    let is_map_object = obj
        .get("additionalProperties")
        .is_some_and(JsonValue::is_object)
        && !has_properties;

    if is_map_object {
        return;
    }

    if !obj.contains_key("type") && has_properties {
        obj.insert("type".to_string(), JsonValue::String("object".to_string()));
    }

    obj.insert("additionalProperties".to_string(), JsonValue::Bool(false));

    if !obj.contains_key("properties") {
        obj.insert("properties".to_string(), JsonValue::Object(Map::new()));
    }

    let current_required: HashSet<String> = obj
        .get("required")
        .and_then(|r| r.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default();

    let all_keys: Vec<String> = obj
        .get("properties")
        .and_then(|p| p.as_object())
        .map(|props| {
            let mut keys: Vec<String> = props.keys().cloned().collect();
            keys.sort();
            keys
        })
        .unwrap_or_default();

    if let Some(JsonValue::Object(props)) = obj.get_mut("properties") {
        for key in &all_keys {
            if let Some(prop_schema) = props.get_mut(key) {
                normalize_schema_recursive(prop_schema);
            }
            if !current_required.contains(key)
                && let Some(prop_schema) = props.get_mut(key)
            {
                make_nullable(prop_schema);
            }
        }
    }

    let required_value: Vec<JsonValue> = all_keys.into_iter().map(JsonValue::String).collect();
    obj.insert("required".to_string(), JsonValue::Array(required_value));
}

fn make_nullable(schema: &mut JsonValue) {
    let obj = match schema.as_object_mut() {
        Some(o) => o,
        None => return,
    };

    if let Some(type_val) = obj.get("type").cloned() {
        match type_val {
            JsonValue::String(ref t) if t != "null" => {
                obj.insert("type".to_string(), serde_json::json!([t, "null"]));
            }
            JsonValue::Array(ref arr) => {
                let has_null = arr.iter().any(|v| v.as_str() == Some("null"));
                if !has_null {
                    let mut new_arr = arr.clone();
                    new_arr.push(JsonValue::String("null".to_string()));
                    obj.insert("type".to_string(), JsonValue::Array(new_arr));
                }
            }
            _ => {}
        }
    } else {
        let existing = JsonValue::Object(obj.clone());
        obj.clear();
        obj.insert(
            "anyOf".to_string(),
            serde_json::json!([existing, {"type": "null"}]),
        );
    }
}
