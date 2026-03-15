//! OpenAI strict-mode schema normalization for provider-bound tool schemas.
//!
//! This module rewrites generic JSON Schema into the stricter shape required by
//! OpenAI function calling. It flattens forbidden top-level variants, preserves
//! typed maps, and recursively normalizes nested object and array properties
//! without mutating the original tool definition.

mod merge;
mod recursive;

use serde_json::Value as JsonValue;

use self::merge::flatten_top_level_forbidden_keywords;
use self::recursive::normalize_schema_recursive;

/// Normalize a JSON Schema for OpenAI strict mode compliance.
pub(crate) fn normalize_schema_strict(schema: &JsonValue) -> JsonValue {
    let mut schema = schema.clone();
    flatten_top_level_forbidden_keywords(&mut schema);
    normalize_schema_recursive(&mut schema);
    schema
}

#[cfg(test)]
mod tests;
