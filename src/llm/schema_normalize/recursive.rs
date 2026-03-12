//! Recursive JSON Schema normalization for nested properties, combinators, and
//! array items before strict validation and top-level variant merging.

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

    if is_map_object(obj, has_properties) {
        return;
    }

    if !obj.contains_key("type") && has_properties {
        obj.insert("type".to_string(), JsonValue::String("object".to_string()));
    }

    obj.insert("additionalProperties".to_string(), JsonValue::Bool(false));

    if !obj.contains_key("properties") {
        obj.insert("properties".to_string(), JsonValue::Object(Map::new()));
    }

    let current_required_values: Vec<String> = obj
        .get("required")
        .and_then(|r| r.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(JsonValue::as_str)
                .map(ToOwned::to_owned)
                .collect()
        })
        .unwrap_or_default();
    let current_required: HashSet<&str> =
        current_required_values.iter().map(String::as_str).collect();

    if let Some(JsonValue::Object(props)) = obj.get_mut("properties") {
        let mut all_keys = Vec::with_capacity(props.len());
        for (key, prop_schema) in props.iter_mut() {
            all_keys.push(key.clone());
            normalize_schema_recursive(prop_schema);
            if !current_required.contains(key.as_str()) {
                make_nullable(prop_schema);
            }
        }

        all_keys.sort();
        let required_value: Vec<JsonValue> = all_keys.into_iter().map(JsonValue::String).collect();
        obj.insert("required".to_string(), JsonValue::Array(required_value));
    }
}

fn is_map_object(obj: &Map<String, JsonValue>, has_properties: bool) -> bool {
    obj.get("additionalProperties")
        .is_some_and(JsonValue::is_object)
        && !has_properties
}

fn make_nullable(schema: &mut JsonValue) {
    let Some(obj) = schema.as_object_mut() else {
        return;
    };

    let Some(type_val) = obj.get_mut("type") else {
        let existing = JsonValue::Object(obj.clone());
        obj.clear();
        obj.insert(
            "anyOf".to_string(),
            serde_json::json!([existing, {"type": "null"}]),
        );
        return;
    };

    match type_val {
        JsonValue::String(t) => {
            if t == "null" {
                return;
            }
            let current = std::mem::take(t);
            *type_val = JsonValue::Array(vec![
                JsonValue::String(current),
                JsonValue::String("null".to_string()),
            ]);
        }
        JsonValue::Array(arr) => {
            if arr.iter().any(|v| v.as_str() == Some("null")) {
                return;
            }
            arr.push(JsonValue::String("null".to_string()));
        }
        _ => {}
    }
}
