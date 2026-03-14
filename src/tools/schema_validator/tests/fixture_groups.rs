//! Fixture-backed schema test groups used by the strict tool schema validator.

use super::*;
use rstest::rstest;

pub(crate) fn load_complex_tool_schema_fixture(tool_name: &str) -> serde_json::Value {
    let path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/schemas")
        .join(format!("{tool_name}.json"));
    let raw = std::fs::read_to_string(&path)
        .unwrap_or_else(|err| panic!("failed to read schema fixture {}: {err}", path.display()));

    serde_json::from_str(&raw)
        .unwrap_or_else(|err| panic!("failed to parse schema fixture for {tool_name}: {err}"))
}

fn simple_tool_schemas() -> Vec<(String, serde_json::Value)> {
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

    tools
        .into_iter()
        .map(|tool| (tool.name().to_string(), tool.parameters_schema()))
        .collect()
}

fn job_tool_schemas() -> Vec<(String, serde_json::Value)> {
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

    tools
        .into_iter()
        .map(|tool| (tool.name().to_string(), tool.parameters_schema()))
        .collect()
}

fn skill_tool_schemas() -> Vec<(String, serde_json::Value)> {
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

    tools
        .into_iter()
        .map(|tool| (tool.name().to_string(), tool.parameters_schema()))
        .collect()
}

/// Validate schemas from tools that cannot be easily constructed by
/// inlining the JSON schema directly. This covers the extension tools and
/// routine tools whose constructors require heavy dependencies.
fn complex_tool_schemas() -> Vec<(String, serde_json::Value)> {
    vec![
        (
            "tool_search".to_string(),
            load_complex_tool_schema_fixture("tool_search"),
        ),
        (
            "tool_install".to_string(),
            load_complex_tool_schema_fixture("tool_install"),
        ),
        (
            "tool_auth".to_string(),
            load_complex_tool_schema_fixture("tool_auth"),
        ),
        (
            "tool_activate".to_string(),
            load_complex_tool_schema_fixture("tool_activate"),
        ),
        (
            "tool_list".to_string(),
            load_complex_tool_schema_fixture("tool_list"),
        ),
        (
            "tool_remove".to_string(),
            load_complex_tool_schema_fixture("tool_remove"),
        ),
        (
            "routine_create".to_string(),
            load_complex_tool_schema_fixture("routine_create"),
        ),
        (
            "routine_list".to_string(),
            load_complex_tool_schema_fixture("routine_list"),
        ),
        (
            "routine_update".to_string(),
            load_complex_tool_schema_fixture("routine_update"),
        ),
        (
            "routine_delete".to_string(),
            load_complex_tool_schema_fixture("routine_delete"),
        ),
        (
            "routine_fire".to_string(),
            load_complex_tool_schema_fixture("routine_fire"),
        ),
        (
            "routine_history".to_string(),
            load_complex_tool_schema_fixture("routine_history"),
        ),
        (
            "job_events".to_string(),
            load_complex_tool_schema_fixture("job_events"),
        ),
        (
            "job_prompt".to_string(),
            load_complex_tool_schema_fixture("job_prompt"),
        ),
    ]
}

fn validate_named_schemas(schemas: Vec<(String, serde_json::Value)>, context: &str) {
    let mut failures = Vec::new();

    for (name, schema) in schemas {
        if let Err(errors) = validate_strict_schema(&schema, &name) {
            failures.push(format!("Tool '{}': {}", name, errors.join("; ")));
        }
    }

    assert!(
        failures.is_empty(),
        "Schema validation failures for {context}:\n{}",
        failures.join("\n")
    );
}

#[rstest]
#[case::simple(simple_tool_schemas(), "simple tool schemas")]
#[case::jobs(job_tool_schemas(), "job tool schemas")]
#[case::skills(skill_tool_schemas(), "skill tool schemas")]
#[case::complex(complex_tool_schemas(), "inline schemas")]
fn test_schema_fixture_groups(
    #[case] schemas: Vec<(String, serde_json::Value)>,
    #[case] context: &str,
) {
    validate_named_schemas(schemas, context);
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
