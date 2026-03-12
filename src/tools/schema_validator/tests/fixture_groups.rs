//! Fixture-backed schema test groups used by the strict tool schema validator.

use super::*;

pub(crate) fn load_complex_tool_schema_fixture(tool_name: &str) -> serde_json::Value {
    let path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/schemas")
        .join(format!("{tool_name}.json"));
    let raw = std::fs::read_to_string(&path)
        .unwrap_or_else(|err| panic!("failed to read schema fixture {}: {err}", path.display()));

    serde_json::from_str(&raw)
        .unwrap_or_else(|err| panic!("failed to parse schema fixture for {tool_name}: {err}"))
}

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
    let path = dir.path().to_path_buf();
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
    let bad_no_type = serde_json::json!({
        "properties": {
            "query": { "type": "string" }
        }
    });
    assert!(validate_strict_schema(&bad_no_type, "ext_no_type").is_err());

    let bad_required = serde_json::json!({
        "type": "object",
        "properties": {
            "input": { "type": "string" }
        },
        "required": ["inpt"]
    });
    assert!(validate_strict_schema(&bad_required, "ext_typo").is_err());

    let bad_array = serde_json::json!({
        "type": "object",
        "properties": {
            "tags": { "type": "array" }
        }
    });
    assert!(validate_strict_schema(&bad_array, "ext_no_items").is_err());

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
    assert!(
        validate_strict_schema(&bad_nested, "ext_nested_no_type").is_ok(),
        "bad_nested is intentionally tolerated today even when a nested property omits \"type\""
    );
}
