//! Schema-merging utilities that flatten top-level variant keywords and
//! consolidate property metadata across `oneOf`/`anyOf`/`allOf` branches.

use std::collections::BTreeSet;

use serde_json::{Map, Value as JsonValue};

#[derive(Clone, Copy)]
enum RequiredMergeMode {
    Intersect,
    Union,
}

pub(super) fn flatten_top_level_forbidden_keywords(schema: &mut JsonValue) {
    let Some(obj) = schema.as_object_mut() else {
        return;
    };

    if let Some(one_of) = obj.remove("oneOf") {
        merge_top_level_variants(obj, &one_of, RequiredMergeMode::Intersect);
    }
    if let Some(any_of) = obj.remove("anyOf") {
        merge_top_level_variants(obj, &any_of, RequiredMergeMode::Intersect);
    }
    if let Some(all_of) = obj.remove("allOf") {
        merge_top_level_variants(obj, &all_of, RequiredMergeMode::Union);
    }

    // OpenAI rejects these keywords at the top-level tool schema. When they
    // appear here, the safest provider-boundary fallback is to drop them and
    // keep the normalized object properties that the model can still use.
    obj.remove("enum");
    obj.remove("not");
}

/// `merge_top_level_variants` preserves the parent combinator's required-key
/// semantics while flattening top-level variant branches.
///
/// When `required_merge_mode` is `RequiredMergeMode::Union` (`allOf`), required
/// keys are unioned because every branch must be satisfied. When
/// `required_merge_mode` is `RequiredMergeMode::Intersect` (`oneOf`/`anyOf`),
/// required keys are intersected because any single branch may apply.
fn merge_top_level_variants(
    root: &mut Map<String, JsonValue>,
    variants_value: &JsonValue,
    required_merge_mode: RequiredMergeMode,
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
                    Some(existing) => {
                        merge_property_schema(existing, schema, required_merge_mode);
                    }
                    None => {
                        props.insert(name.clone(), schema.clone());
                    }
                }
            }
        }
        variant_required_sets.push(required_keys(variant_obj.get("required")));
    }

    let variant_required = match required_merge_mode {
        RequiredMergeMode::Union => union_required_keys(&variant_required_sets),
        RequiredMergeMode::Intersect => intersect_required_keys(&variant_required_sets),
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

fn merge_property_schema(
    existing: &mut JsonValue,
    incoming: &JsonValue,
    required_merge_mode: RequiredMergeMode,
) {
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
                if merge_object_property_schema(existing_obj, incoming_obj, required_merge_mode) {
                    return;
                }
            }
            Some("array") => {
                if merge_array_property_schema(existing_obj, incoming_obj, required_merge_mode) {
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
    required_merge_mode: RequiredMergeMode,
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
                    Some(existing_schema) => {
                        merge_property_schema(existing_schema, schema, required_merge_mode);
                    }
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

    merge_object_required(existing_obj, incoming_obj, required_merge_mode);

    merge_schema_field(
        existing_obj,
        incoming_obj,
        "additionalProperties",
        required_merge_mode,
    );

    true
}

fn merge_object_required(
    existing_obj: &mut Map<String, JsonValue>,
    incoming_obj: &Map<String, JsonValue>,
    required_merge_mode: RequiredMergeMode,
) {
    if !existing_obj.contains_key("required") && !incoming_obj.contains_key("required") {
        return;
    }

    let required = match required_merge_mode {
        RequiredMergeMode::Intersect => intersect_required_keys(&[
            required_keys(existing_obj.get("required")),
            required_keys(incoming_obj.get("required")),
        ]),
        RequiredMergeMode::Union => union_required_keys(&[
            required_keys(existing_obj.get("required")),
            required_keys(incoming_obj.get("required")),
        ]),
    };

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
    required_merge_mode: RequiredMergeMode,
) -> bool {
    merge_schema_field(existing_obj, incoming_obj, "items", required_merge_mode);
    true
}

fn merge_schema_field(
    existing_obj: &mut Map<String, JsonValue>,
    incoming_obj: &Map<String, JsonValue>,
    key: &str,
    required_merge_mode: RequiredMergeMode,
) {
    match (existing_obj.get_mut(key), incoming_obj.get(key)) {
        (Some(existing_value), Some(incoming_value)) => {
            if existing_value.is_object() && incoming_value.is_object() {
                merge_property_schema(existing_value, incoming_value, required_merge_mode);
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
            // These linear scans keep the logic simple, but the duplicate
            // checks are O(n^2) overall. If profiling ever shows large variant
            // arrays or this becomes a hot path, switch to HashSet-backed
            // deduplication instead of repeated `iter().any(...)` checks.
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
