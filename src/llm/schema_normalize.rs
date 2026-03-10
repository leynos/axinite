use std::collections::{BTreeSet, HashSet};

use serde_json::{Map, Value as JsonValue};

/// Normalize a JSON Schema for OpenAI strict mode compliance.
///
/// OpenAI strict function calling requires:
/// - Every fixed-shape object must have `"additionalProperties": false`
/// - `"required"` must list ALL property keys
/// - Optional fields use `"type": ["<original>", "null"]` instead of being
///   omitted from `required`
/// - Nested objects and array items are recursively normalized
/// - Typed map objects keep their `"additionalProperties"` schema instead of
///   being rewritten into fixed-shape objects
///
/// This is applied as a clone-and-transform at the provider boundary so the
/// original tool definitions remain unchanged for other providers.
pub(crate) fn normalize_schema_strict(schema: &JsonValue) -> JsonValue {
    let mut schema = schema.clone();
    flatten_top_level_forbidden_keywords(&mut schema);
    normalize_schema_recursive(&mut schema);
    schema
}

fn flatten_top_level_forbidden_keywords(schema: &mut JsonValue) {
    let Some(obj) = schema.as_object_mut() else {
        return;
    };

    if let Some(one_of) = obj.remove("oneOf") {
        merge_top_level_variants(obj, &one_of, false);
    }
    if let Some(any_of) = obj.remove("anyOf") {
        merge_top_level_variants(obj, &any_of, false);
    }
    if let Some(all_of) = obj.remove("allOf") {
        merge_top_level_variants(obj, &all_of, true);
    }

    // OpenAI rejects these keywords at the top-level tool schema. When they
    // appear here, the safest provider-boundary fallback is to drop them and
    // keep the normalized object properties that the model can still use.
    obj.remove("enum");
    obj.remove("not");
}

fn merge_top_level_variants(
    root: &mut Map<String, JsonValue>,
    variants_value: &JsonValue,
    require_across_all_variants: bool,
) {
    let Some(variants) = variants_value.as_array() else {
        return;
    };

    let mut root_required = required_keys(root.get("required"));
    let mut variant_required_sets: Vec<BTreeSet<String>> = Vec::new();

    let props_value = root
        .entry("properties".to_string())
        .or_insert_with(|| JsonValue::Object(Map::new()));
    if !props_value.is_object() {
        *props_value = JsonValue::Object(Map::new());
    }
    let Some(props) = props_value.as_object_mut() else {
        return;
    };

    for variant in variants {
        let Some(variant_obj) = variant.as_object() else {
            continue;
        };
        if let Some(variant_props) = variant_obj.get("properties").and_then(JsonValue::as_object) {
            for (name, schema) in variant_props {
                match props.get_mut(name) {
                    Some(existing) => merge_property_schema(existing, schema),
                    None => {
                        props.insert(name.clone(), schema.clone());
                    }
                }
            }
        }
        variant_required_sets.push(required_keys(variant_obj.get("required")));
    }

    let variant_required = if require_across_all_variants {
        union_required_keys(&variant_required_sets)
    } else {
        intersect_required_keys(&variant_required_sets)
    };
    root_required.extend(variant_required);

    if root_required.is_empty() {
        root.remove("required");
    } else {
        root.insert(
            "required".to_string(),
            JsonValue::Array(root_required.into_iter().map(JsonValue::String).collect()),
        );
    }
}

fn required_keys(value: Option<&JsonValue>) -> BTreeSet<String> {
    value
        .and_then(JsonValue::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(|item| item.as_str().map(ToOwned::to_owned))
                .collect()
        })
        .unwrap_or_default()
}

fn intersect_required_keys(sets: &[BTreeSet<String>]) -> BTreeSet<String> {
    let Some((first, rest)) = sets.split_first() else {
        return BTreeSet::new();
    };
    rest.iter().fold(first.clone(), |acc, set| {
        acc.intersection(set).cloned().collect()
    })
}

fn union_required_keys(sets: &[BTreeSet<String>]) -> BTreeSet<String> {
    sets.iter().fold(BTreeSet::new(), |mut acc, set| {
        acc.extend(set.iter().cloned());
        acc
    })
}

fn merge_property_schema(existing: &mut JsonValue, incoming: &JsonValue) {
    if existing == incoming {
        return;
    }

    if let Some(merged) = merge_string_literal_property(existing, incoming) {
        *existing = merged;
        return;
    }

    if let (Some(existing_obj), Some(incoming_obj)) =
        (existing.as_object_mut(), incoming.as_object())
        && existing_obj.get("type") == incoming_obj.get("type")
    {
        merge_common_schema_metadata(existing_obj, incoming_obj);

        match existing_obj.get("type").and_then(JsonValue::as_str) {
            Some("object") => {
                if merge_object_property_schema(existing_obj, incoming_obj) {
                    return;
                }
            }
            Some("array") => {
                if merge_array_property_schema(existing_obj, incoming_obj) {
                    return;
                }
            }
            _ => return,
        }
    }

    *existing = merge_nested_any_of(existing.clone(), incoming.clone());
}

fn merge_common_schema_metadata(
    existing_obj: &mut Map<String, JsonValue>,
    incoming_obj: &Map<String, JsonValue>,
) {
    if !existing_obj.contains_key("description")
        && let Some(description) = incoming_obj.get("description")
    {
        existing_obj.insert("description".to_string(), description.clone());
    }
    if !existing_obj.contains_key("default")
        && let Some(default) = incoming_obj.get("default")
    {
        existing_obj.insert("default".to_string(), default.clone());
    }
}

fn merge_object_property_schema(
    existing_obj: &mut Map<String, JsonValue>,
    incoming_obj: &Map<String, JsonValue>,
) -> bool {
    match (
        existing_obj.get_mut("properties"),
        incoming_obj.get("properties"),
    ) {
        (Some(existing_props_value), Some(incoming_props_value)) => {
            let Some(existing_props) = existing_props_value.as_object_mut() else {
                return false;
            };
            let Some(incoming_props) = incoming_props_value.as_object() else {
                return false;
            };
            for (name, schema) in incoming_props {
                match existing_props.get_mut(name) {
                    Some(existing_schema) => merge_property_schema(existing_schema, schema),
                    None => {
                        existing_props.insert(name.clone(), schema.clone());
                    }
                }
            }
        }
        (None, Some(incoming_props_value)) => {
            if !incoming_props_value.is_object() {
                return false;
            }
            existing_obj.insert("properties".to_string(), incoming_props_value.clone());
        }
        _ => {}
    }

    merge_object_required(existing_obj, incoming_obj);

    if !merge_schema_field(existing_obj, incoming_obj, "additionalProperties") {
        return false;
    }

    true
}

fn merge_object_required(
    existing_obj: &mut Map<String, JsonValue>,
    incoming_obj: &Map<String, JsonValue>,
) {
    if !existing_obj.contains_key("required") && !incoming_obj.contains_key("required") {
        return;
    }

    let required = intersect_required_keys(&[
        required_keys(existing_obj.get("required")),
        required_keys(incoming_obj.get("required")),
    ]);

    if required.is_empty() {
        existing_obj.remove("required");
        return;
    }

    existing_obj.insert(
        "required".to_string(),
        JsonValue::Array(required.into_iter().map(JsonValue::String).collect()),
    );
}

fn merge_array_property_schema(
    existing_obj: &mut Map<String, JsonValue>,
    incoming_obj: &Map<String, JsonValue>,
) -> bool {
    merge_schema_field(existing_obj, incoming_obj, "items")
}

fn merge_schema_field(
    existing_obj: &mut Map<String, JsonValue>,
    incoming_obj: &Map<String, JsonValue>,
    key: &str,
) -> bool {
    match (existing_obj.get_mut(key), incoming_obj.get(key)) {
        (Some(existing_value), Some(incoming_value)) => {
            if existing_value.is_object() && incoming_value.is_object() {
                merge_property_schema(existing_value, incoming_value);
            } else if existing_value != incoming_value {
                *existing_value =
                    merge_nested_any_of(existing_value.clone(), incoming_value.clone());
            }
        }
        (None, Some(incoming_value)) => {
            existing_obj.insert(key.to_string(), incoming_value.clone());
        }
        _ => {}
    }
    true
}

fn merge_string_literal_property(existing: &JsonValue, incoming: &JsonValue) -> Option<JsonValue> {
    let mut values = string_literal_values(existing)?;
    values.extend(string_literal_values(incoming)?);

    let mut merged = Map::new();
    merged.insert("type".to_string(), JsonValue::String("string".to_string()));
    merged.insert(
        "enum".to_string(),
        JsonValue::Array(values.into_iter().map(JsonValue::String).collect()),
    );
    if let Some(description) = first_description(existing, incoming) {
        merged.insert("description".to_string(), JsonValue::String(description));
    }
    Some(JsonValue::Object(merged))
}

fn string_literal_values(schema: &JsonValue) -> Option<BTreeSet<String>> {
    let obj = schema.as_object()?;
    let mut values = BTreeSet::new();

    if let Some(value) = obj.get("const").and_then(JsonValue::as_str) {
        values.insert(value.to_string());
    }

    if let Some(items) = obj.get("enum").and_then(JsonValue::as_array) {
        for item in items {
            let value = item.as_str()?;
            values.insert(value.to_string());
        }
    }

    (!values.is_empty()).then_some(values)
}

fn first_description(existing: &JsonValue, incoming: &JsonValue) -> Option<String> {
    existing
        .get("description")
        .and_then(JsonValue::as_str)
        .or_else(|| incoming.get("description").and_then(JsonValue::as_str))
        .map(ToOwned::to_owned)
}

fn merge_nested_any_of(existing: JsonValue, incoming: JsonValue) -> JsonValue {
    let mut variants = Vec::new();
    collect_any_of_variants(existing, &mut variants);
    collect_any_of_variants(incoming, &mut variants);
    JsonValue::Object(Map::from_iter([(
        "anyOf".to_string(),
        JsonValue::Array(variants),
    )]))
}

fn collect_any_of_variants(value: JsonValue, variants: &mut Vec<JsonValue>) {
    if let Some(existing_any_of) = value.get("anyOf").and_then(JsonValue::as_array) {
        for variant in existing_any_of {
            if !variants.iter().any(|candidate| candidate == variant) {
                variants.push(variant.clone());
            }
        }
        return;
    }

    if !variants.iter().any(|candidate| candidate == &value) {
        variants.push(value);
    }
}

fn normalize_schema_recursive(schema: &mut JsonValue) {
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

#[cfg(test)]
mod tests {
    use super::normalize_schema_strict;
    use serde_json::Value as JsonValue;

    fn github_style_schema() -> JsonValue {
        serde_json::json!({
            "type": "object",
            "required": ["action"],
            "oneOf": [
                {
                    "properties": {
                        "action": { "const": "get_repo" },
                        "owner": { "type": "string" },
                        "repo": { "type": "string" }
                    },
                    "required": ["action", "owner", "repo"]
                },
                {
                    "properties": {
                        "action": { "const": "create_issue" },
                        "owner": { "type": "string" },
                        "repo": { "type": "string" },
                        "title": { "type": "string" }
                    },
                    "required": ["action", "owner", "repo", "title"]
                }
            ]
        })
    }

    #[test]
    fn test_normalize_schema_strict_flattens_top_level_oneof() {
        let normalized = normalize_schema_strict(&github_style_schema());

        assert_eq!(normalized["type"], serde_json::json!("object"));
        assert!(
            normalized.get("oneOf").is_none(),
            "top-level oneOf must be removed for OpenAI compatibility: {normalized}"
        );
        assert!(
            normalized.get("anyOf").is_none(),
            "top-level anyOf must be removed for OpenAI compatibility: {normalized}"
        );
        assert_eq!(
            normalized["properties"]["action"]["enum"],
            serde_json::json!(["create_issue", "get_repo"])
        );
        assert_eq!(
            normalized["properties"]["owner"]["type"],
            serde_json::json!("string")
        );
        assert_eq!(
            normalized["properties"]["title"]["type"],
            serde_json::json!(["string", "null"])
        );
    }

    #[test]
    fn test_normalize_schema_strict_preserves_typed_map_objects() {
        let normalized = normalize_schema_strict(&serde_json::json!({
            "type": "object",
            "properties": {
                "action": { "type": "string" },
                "inputs": {
                    "type": "object",
                    "additionalProperties": { "type": "string" }
                }
            },
            "required": ["action"]
        }));

        assert_eq!(
            normalized["properties"]["inputs"]["additionalProperties"],
            serde_json::json!({ "type": "string" })
        );
        assert!(
            normalized["properties"]["inputs"]
                .get("properties")
                .is_none(),
            "typed map objects should not be rewritten into empty fixed-shape objects: {normalized}"
        );
    }

    #[test]
    fn test_normalize_schema_strict_merges_shared_nested_object_properties() {
        let normalized = normalize_schema_strict(&serde_json::json!({
            "type": "object",
            "required": ["action"],
            "oneOf": [
                {
                    "properties": {
                        "action": { "const": "first" },
                        "inputs": {
                            "type": "object",
                            "properties": {
                                "owner": { "type": "string" }
                            },
                            "required": ["owner"],
                            "additionalProperties": false
                        }
                    },
                    "required": ["action", "inputs"]
                },
                {
                    "properties": {
                        "action": { "const": "second" },
                        "inputs": {
                            "type": "object",
                            "properties": {
                                "repo": { "type": "string" }
                            },
                            "required": ["repo"],
                            "additionalProperties": false
                        }
                    },
                    "required": ["action", "inputs"]
                }
            ]
        }));

        assert!(
            normalized["properties"]["inputs"]["properties"]["owner"].is_object(),
            "expected first nested property to survive merge: {normalized}"
        );
        assert!(
            normalized["properties"]["inputs"]["properties"]["repo"].is_object(),
            "expected later nested property to survive merge: {normalized}"
        );
    }
}
