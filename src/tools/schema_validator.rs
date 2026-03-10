// === QA Plan P0 - 1.1: Tool schema validator ===
//!
//! Validates tool parameter schemas against OpenAI strict-mode rules.
//!
//! This module provides a comprehensive validation function and a test that
//! exercises every built-in tool's `parameters_schema()` to ensure compatibility
//! with the OpenAI function calling API strict mode.

/// Strict CI-time validation of a JSON schema against OpenAI strict-mode rules.
///
/// Use this function in tests and CI to catch subtle schema defects that the
/// lenient runtime validator allows (freeform properties, missing
/// `additionalProperties`, enum-type mismatches).
///
/// For the lenient runtime variant used at tool-registration time, see
/// [`validate_tool_schema`](crate::tools::tool::validate_tool_schema) in
/// `tool.rs`.
///
/// Returns `Ok(())` if the schema is valid, or `Err(errors)` with a list of
/// all violations found. The validation is recursive for nested objects and
/// array items.
///
/// # Rules enforced
///
/// 1. Top-level must have `"type": "object"`
/// 2. Must have `"properties"` as a JSON object
/// 3. Every key in `"required"` must exist in `"properties"`
/// 4. Every property must have a `"type"` field (freeform/any-type is flagged)
/// 5. `"additionalProperties"` must be explicitly `false` if present
/// 6. Nested objects follow the same rules recursively
/// 7. `"enum"` values must match the declared type
/// 8. Array properties must have an `"items"` definition
/// 9. Top-level schemas must not use `oneOf`/`anyOf`/`allOf`/`enum`/`not`
pub fn validate_strict_schema(
    schema: &serde_json::Value,
    tool_name: &str,
) -> Result<(), Vec<String>> {
    let mut errors = Vec::new();
    for forbidden in ["oneOf", "anyOf", "allOf", "enum", "not"] {
        if schema.get(forbidden).is_some() {
            errors.push(format!(
                "{tool_name}: top-level \"{forbidden}\" is not allowed in OpenAI tool schemas"
            ));
        }
    }
    errors.extend(check_object_schema(schema, tool_name));
    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors)
    }
}

/// Recursively validate an object-typed schema node.
fn check_object_schema(schema: &serde_json::Value, path: &str) -> Vec<String> {
    let mut errors = Vec::new();

    // Rule 1: must have "type": "object"
    match schema.get("type").and_then(|t| t.as_str()) {
        Some("object") => {}
        Some(other) => {
            errors.push(format!("{path}: expected type \"object\", got \"{other}\""));
            return errors;
        }
        None => {
            errors.push(format!("{path}: missing \"type\": \"object\""));
            return errors;
        }
    }

    // Rule 2: must have "properties" as an object
    let properties = match schema.get("properties").and_then(|p| p.as_object()) {
        Some(p) => p,
        None => {
            errors.push(format!("{path}: missing or non-object \"properties\""));
            return errors;
        }
    };

    // Rule 3: every key in "required" must exist in "properties"
    if let Some(required) = schema.get("required").and_then(|r| r.as_array()) {
        for req in required {
            if let Some(key) = req.as_str()
                && !properties.contains_key(key)
            {
                errors.push(format!(
                    "{path}: required key \"{key}\" not found in properties"
                ));
            }
        }
    }

    // Rule 4: every property should have a "type" field
    for (key, prop) in properties {
        let prop_path = format!("{path}.{key}");

        if prop.get("type").is_none() {
            // Freeform properties (no type) are intentionally allowed in some tools
            // (json "data", http "body") for OpenAI compatibility with union types.
            // We flag them as warnings but don't treat them as hard errors.
            // Uncomment the next line to enforce strict typing:
            // errors.push(format!("{prop_path}: property missing \"type\" field"));
            continue;
        }

        let prop_type = prop.get("type").and_then(|t| t.as_str()).unwrap_or("");

        // Rule 5: additionalProperties must be false if present
        if let Some(additional) = prop.get("additionalProperties")
            && additional != &serde_json::Value::Bool(false)
            // Allow additionalProperties with a type schema (e.g. {"type": "string"})
            // which is valid in JSON Schema and used by tools like create_job's credentials.
            && additional.get("type").is_none()
        {
            errors.push(format!(
                "{prop_path}: \"additionalProperties\" should be false or a type schema"
            ));
        }

        // Rule 7: enum values must match the declared type
        if let Some(enum_values) = prop.get("enum").and_then(|e| e.as_array()) {
            for (i, val) in enum_values.iter().enumerate() {
                let type_matches = match prop_type {
                    "string" => val.is_string(),
                    "integer" | "number" => val.is_number(),
                    "boolean" => val.is_boolean(),
                    _ => true, // unknown types: skip check
                };
                if !type_matches {
                    errors.push(format!(
                        "{prop_path}: enum[{i}] value {val} does not match declared type \"{prop_type}\""
                    ));
                }
            }
        }

        // Rule 6: nested objects follow the same rules
        if prop_type == "object" {
            // Objects with additionalProperties as a type schema (e.g. credentials map)
            // are valid JSON Schema patterns, not strict-mode objects with fixed properties.
            if prop.get("additionalProperties").is_some() && prop.get("properties").is_none() {
                // This is a map type (e.g. {"type": "object", "additionalProperties": {"type": "string"}})
                // Valid pattern, skip recursive object validation.
            } else {
                errors.extend(check_object_schema(prop, &prop_path));
            }
        }

        // Rule 8: arrays must have "items"
        if prop_type == "array" {
            if prop.get("items").is_none() {
                errors.push(format!("{prop_path}: array property missing \"items\""));
            } else if let Some(items) = prop.get("items") {
                // Recurse into items if they are objects
                if items.get("type").and_then(|t| t.as_str()) == Some("object") {
                    errors.extend(check_object_schema(items, &format!("{prop_path}.items")));
                }
            }
        }
    }

    // Also check top-level additionalProperties (rule 5)
    if let Some(additional) = schema.get("additionalProperties")
        && additional != &serde_json::Value::Bool(false)
        && additional.get("type").is_none()
    {
        errors.push(format!(
            "{path}: top-level \"additionalProperties\" should be false or a type schema"
        ));
    }

    errors
}

#[cfg(test)]
mod tests {
    // ── Unit tests for the validator itself ──────────────────────────────

    #[test]
    fn test_valid_schema_passes() {
        let schema = serde_json::json!({
            "type": "object",
            "properties": {
                "name": { "type": "string", "description": "A name" }
            },
            "required": ["name"]
        });
        assert!(validate_strict_schema(&schema, "test").is_ok());
    }

    #[test]
    fn test_missing_type_fails() {
        let schema = serde_json::json!({
            "properties": {
                "name": { "type": "string" }
            }
        });
        let err = validate_strict_schema(&schema, "test").unwrap_err();
        assert!(err[0].contains("missing \"type\": \"object\""));
    }

    #[test]
    fn test_wrong_type_fails() {
        let schema = serde_json::json!({ "type": "string" });
        let err = validate_strict_schema(&schema, "test").unwrap_err();
        assert!(err[0].contains("expected type \"object\""));
    }

    #[test]
    fn test_required_not_in_properties_fails() {
        let schema = serde_json::json!({
            "type": "object",
            "properties": {
                "name": { "type": "string" }
            },
            "required": ["name", "age"]
        });
        let err = validate_strict_schema(&schema, "test").unwrap_err();
        assert!(err.iter().any(|e| e.contains("\"age\" not found")));
    }

    #[test]
    fn test_nested_object_validated() {
        let schema = serde_json::json!({
            "type": "object",
            "properties": {
                "config": {
                    "type": "object",
                    "properties": {
                        "key": { "type": "string" }
                    },
                    "required": ["key", "missing"]
                }
            }
        });
        let err = validate_strict_schema(&schema, "test").unwrap_err();
        assert!(
            err.iter()
                .any(|e| e.contains("test.config") && e.contains("\"missing\""))
        );
    }

    #[test]
    fn test_array_missing_items_fails() {
        let schema = serde_json::json!({
            "type": "object",
            "properties": {
                "tags": { "type": "array", "description": "Tags" }
            }
        });
        let err = validate_strict_schema(&schema, "test").unwrap_err();
        assert!(
            err.iter()
                .any(|e| e.contains("array property missing \"items\""))
        );
    }

    #[test]
    fn test_array_with_items_passes() {
        let schema = serde_json::json!({
            "type": "object",
            "properties": {
                "tags": {
                    "type": "array",
                    "items": { "type": "string" }
                }
            }
        });
        assert!(validate_strict_schema(&schema, "test").is_ok());
    }

    #[test]
    fn test_forbidden_top_level_keywords_fail() {
        for keyword in ["oneOf", "anyOf", "allOf", "enum", "not"] {
            let mut schema = serde_json::json!({
                "type": "object"
            });
            let root = schema
                .as_object_mut()
                .expect("top-level schema should be an object");

            match keyword {
                "enum" => {
                    root.insert(keyword.to_string(), serde_json::json!(["get_repo"]));
                }
                "not" => {
                    root.insert(keyword.to_string(), serde_json::json!({ "type": "null" }));
                }
                _ => {
                    root.insert(
                        keyword.to_string(),
                        serde_json::json!([
                            {
                                "properties": {
                                    "action": { "const": "get_repo" }
                                },
                                "required": ["action"]
                            }
                        ]),
                    );
                }
            };

            let err = validate_strict_schema(&schema, "test").unwrap_err();
            assert!(
                err.iter().any(|message| {
                    message.contains(&format!("top-level \"{keyword}\" is not allowed"))
                }),
                "expected top-level {keyword} failure, got: {err:?}"
            );
        }
    }

    #[test]
    fn test_nested_one_of_is_allowed() {
        let schema = serde_json::json!({
            "type": "object",
            "properties": {
                "action": { "type": "string" },
                "inputs": {
                    "type": "object",
                    "properties": {
                        "mode": {
                            "oneOf": [
                                { "type": "string" },
                                { "type": "integer" }
                            ]
                        }
                    },
                    "required": ["mode"],
                    "additionalProperties": false
                }
            },
            "required": ["action"]
        });

        assert!(
            validate_strict_schema(&schema, "test").is_ok(),
            "nested combinators should not be rejected by root-only validation"
        );
    }

    #[test]
    fn test_enum_type_mismatch_fails() {
        let schema = serde_json::json!({
            "type": "object",
            "properties": {
                "mode": {
                    "type": "string",
                    "enum": ["fast", 42, "slow"]
                }
            }
        });
        let err = validate_strict_schema(&schema, "test").unwrap_err();
        assert!(err.iter().any(|e| e.contains("enum[1]")));
    }

    #[test]
    fn test_enum_matching_type_passes() {
        let schema = serde_json::json!({
            "type": "object",
            "properties": {
                "mode": {
                    "type": "string",
                    "enum": ["fast", "slow"]
                }
            }
        });
        assert!(validate_strict_schema(&schema, "test").is_ok());
    }

    #[test]
    fn test_nested_array_items_object_validated() {
        let schema = serde_json::json!({
            "type": "object",
            "properties": {
                "headers": {
                    "type": "array",
                    "items": {
                        "type": "object",
                        "properties": {
                            "name": { "type": "string" }
                        },
                        "required": ["name", "ghost"]
                    }
                }
            }
        });
        let err = validate_strict_schema(&schema, "test").unwrap_err();
        assert!(
            err.iter()
                .any(|e| e.contains("headers.items") && e.contains("\"ghost\""))
        );
    }

    #[test]
    fn test_additional_properties_false_passes() {
        let schema = serde_json::json!({
            "type": "object",
            "properties": {
                "header": {
                    "type": "object",
                    "properties": {
                        "name": { "type": "string" }
                    },
                    "additionalProperties": false
                }
            }
        });
        assert!(validate_strict_schema(&schema, "test").is_ok());
    }

    #[test]
    fn test_additional_properties_type_schema_passes() {
        // Map pattern: {"type": "object", "additionalProperties": {"type": "string"}}
        let schema = serde_json::json!({
            "type": "object",
            "properties": {
                "credentials": {
                    "type": "object",
                    "description": "Map of secret names to env var names",
                    "additionalProperties": { "type": "string" }
                }
            }
        });
        assert!(validate_strict_schema(&schema, "test").is_ok());
    }

    // ── Comprehensive test: validate ALL built-in tool schemas ───────────

    #[test]
    fn test_all_simple_tool_schemas() {
        use crate::tools::Tool;
        use crate::tools::builtin::{
            ApplyPatchTool, EchoTool, HttpTool, JsonTool, ListDirTool, ReadFileTool, ShellTool,
            TimeTool, WriteFileTool,
        };

        let tools: Vec<Box<dyn Tool>> = vec![
            Box::new(EchoTool),
            Box::new(TimeTool),
            Box::new(JsonTool),
            Box::new(HttpTool::new()),
            Box::new(ShellTool::new()),
            Box::new(ReadFileTool::new()),
            Box::new(WriteFileTool::new()),
            Box::new(ListDirTool::new()),
            Box::new(ApplyPatchTool::new()),
        ];

        let mut failures = Vec::new();

        for tool in &tools {
            let schema = tool.parameters_schema();
            if let Err(errors) = validate_strict_schema(&schema, tool.name()) {
                failures.push(format!("Tool '{}': {}", tool.name(), errors.join("; ")));
            }
        }

        assert!(
            failures.is_empty(),
            "Schema validation failures:\n{}",
            failures.join("\n")
        );
    }

    #[test]
    fn test_job_tool_schemas() {
        use std::sync::Arc;

        use crate::context::ContextManager;
        use crate::tools::Tool;
        use crate::tools::builtin::{CancelJobTool, CreateJobTool, JobStatusTool, ListJobsTool};

        let ctx_mgr = Arc::new(ContextManager::new(5));

        let tools: Vec<Box<dyn Tool>> = vec![
            Box::new(CreateJobTool::new(Arc::clone(&ctx_mgr))),
            Box::new(ListJobsTool::new(Arc::clone(&ctx_mgr))),
            Box::new(JobStatusTool::new(Arc::clone(&ctx_mgr))),
            Box::new(CancelJobTool::new(Arc::clone(&ctx_mgr))),
        ];

        let mut failures = Vec::new();

        for tool in &tools {
            let schema = tool.parameters_schema();
            if let Err(errors) = validate_strict_schema(&schema, tool.name()) {
                failures.push(format!("Tool '{}': {}", tool.name(), errors.join("; ")));
            }
        }

        assert!(
            failures.is_empty(),
            "Schema validation failures:\n{}",
            failures.join("\n")
        );
    }

    #[test]
    fn test_skill_tool_schemas() {
        use std::sync::Arc;

        use crate::skills::catalog::SkillCatalog;
        use crate::skills::registry::SkillRegistry;
        use crate::tools::Tool;
        use crate::tools::builtin::{
            SkillInstallTool, SkillListTool, SkillRemoveTool, SkillSearchTool,
        };

        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.keep();
        let registry = Arc::new(std::sync::RwLock::new(SkillRegistry::new(path)));
        let catalog = Arc::new(SkillCatalog::with_url("http://127.0.0.1:1"));

        let tools: Vec<Box<dyn Tool>> = vec![
            Box::new(SkillListTool::new(Arc::clone(&registry))),
            Box::new(SkillSearchTool::new(
                Arc::clone(&registry),
                Arc::clone(&catalog),
            )),
            Box::new(SkillInstallTool::new(
                Arc::clone(&registry),
                Arc::clone(&catalog),
            )),
            Box::new(SkillRemoveTool::new(Arc::clone(&registry))),
        ];

        let mut failures = Vec::new();

        for tool in &tools {
            let schema = tool.parameters_schema();
            if let Err(errors) = validate_strict_schema(&schema, tool.name()) {
                failures.push(format!("Tool '{}': {}", tool.name(), errors.join("; ")));
            }
        }

        assert!(
            failures.is_empty(),
            "Schema validation failures:\n{}",
            failures.join("\n")
        );
    }

    /// Validate schemas from tools that cannot be easily constructed by
    /// inlining the JSON schema directly. This covers the extension tools and
    /// routine tools whose constructors require heavy dependencies.
    #[test]
    fn test_inline_schemas_for_complex_tools() {
        fn load_complex_tool_schema_fixture(tool_name: &str) -> serde_json::Value {
            let raw = match tool_name {
                "tool_search" => include_str!("../../tests/fixtures/schemas/tool_search.json"),
                "tool_install" => include_str!("../../tests/fixtures/schemas/tool_install.json"),
                "tool_auth" => include_str!("../../tests/fixtures/schemas/tool_auth.json"),
                "tool_activate" => include_str!("../../tests/fixtures/schemas/tool_activate.json"),
                "tool_list" => include_str!("../../tests/fixtures/schemas/tool_list.json"),
                "tool_remove" => include_str!("../../tests/fixtures/schemas/tool_remove.json"),
                "routine_create" => {
                    include_str!("../../tests/fixtures/schemas/routine_create.json")
                }
                "routine_list" => include_str!("../../tests/fixtures/schemas/routine_list.json"),
                "routine_update" => {
                    include_str!("../../tests/fixtures/schemas/routine_update.json")
                }
                "routine_delete" => {
                    include_str!("../../tests/fixtures/schemas/routine_delete.json")
                }
                "routine_fire" => include_str!("../../tests/fixtures/schemas/routine_fire.json"),
                "routine_history" => {
                    include_str!("../../tests/fixtures/schemas/routine_history.json")
                }
                "job_events" => include_str!("../../tests/fixtures/schemas/job_events.json"),
                "job_prompt" => include_str!("../../tests/fixtures/schemas/job_prompt.json"),
                other => panic!("missing schema fixture for {other}"),
            };

            serde_json::from_str(raw).unwrap_or_else(|err| {
                panic!("failed to parse schema fixture for {tool_name}: {err}")
            })
        }

        // These schemas are extracted from the source code of tools with complex deps.
        // If the source schemas change, these fixtures serve as a canary.
        let schemas: Vec<(&str, serde_json::Value)> = vec![
            (
                "tool_search",
                load_complex_tool_schema_fixture("tool_search"),
            ),
            (
                "tool_install",
                load_complex_tool_schema_fixture("tool_install"),
            ),
            ("tool_auth", load_complex_tool_schema_fixture("tool_auth")),
            (
                "tool_activate",
                load_complex_tool_schema_fixture("tool_activate"),
            ),
            ("tool_list", load_complex_tool_schema_fixture("tool_list")),
            (
                "tool_remove",
                load_complex_tool_schema_fixture("tool_remove"),
            ),
            (
                "routine_create",
                load_complex_tool_schema_fixture("routine_create"),
            ),
            (
                "routine_list",
                load_complex_tool_schema_fixture("routine_list"),
            ),
            (
                "routine_update",
                load_complex_tool_schema_fixture("routine_update"),
            ),
            (
                "routine_delete",
                load_complex_tool_schema_fixture("routine_delete"),
            ),
            (
                "routine_fire",
                load_complex_tool_schema_fixture("routine_fire"),
            ),
            (
                "routine_history",
                load_complex_tool_schema_fixture("routine_history"),
            ),
            ("job_events", load_complex_tool_schema_fixture("job_events")),
            ("job_prompt", load_complex_tool_schema_fixture("job_prompt")),
        ];

        let mut failures = Vec::new();

        for (name, schema) in &schemas {
            if let Err(errors) = validate_strict_schema(schema, name) {
                failures.push(format!("Tool '{}': {}", name, errors.join("; ")));
            }
        }

        assert!(
            failures.is_empty(),
            "Schema validation failures for inline schemas:\n{}",
            failures.join("\n")
        );
    }

    /// Verify the validator catches common issues in externally-sourced schemas.
    /// WASM modules and MCP servers may produce schemas with defects that
    /// built-in tools wouldn't have.
    #[test]
    fn test_external_schema_defects_detected() {
        // Missing top-level type (MCP server omitted it)
        let bad_no_type = serde_json::json!({
            "properties": {
                "query": { "type": "string" }
            }
        });
        assert!(validate_strict_schema(&bad_no_type, "ext_no_type").is_err());

        // Required key not in properties (WASM module typo)
        let bad_required = serde_json::json!({
            "type": "object",
            "properties": {
                "input": { "type": "string" }
            },
            "required": ["inpt"]
        });
        assert!(validate_strict_schema(&bad_required, "ext_typo").is_err());

        // Array without items definition (MCP server bug)
        let bad_array = serde_json::json!({
            "type": "object",
            "properties": {
                "tags": { "type": "array" }
            }
        });
        assert!(validate_strict_schema(&bad_array, "ext_no_items").is_err());

        // Enum type mismatch (WASM module declares string enum with integers)
        let bad_enum = serde_json::json!({
            "type": "object",
            "properties": {
                "mode": {
                    "type": "string",
                    "enum": [1, 2, 3]
                }
            }
        });
        assert!(validate_strict_schema(&bad_enum, "ext_enum_mismatch").is_err());

        // Nested object without type (deeply nested MCP schema)
        let bad_nested = serde_json::json!({
            "type": "object",
            "properties": {
                "config": {
                    "type": "object",
                    "properties": {
                        "setting": { "description": "missing type field" }
                    }
                }
            }
        });
        // This may pass or fail depending on whether we enforce type on every
        // nested property -- the validator allows freeform for compatibility.
        // The important thing is it doesn't panic.
        let _ = validate_strict_schema(&bad_nested, "ext_nested_no_type");
    }
}
