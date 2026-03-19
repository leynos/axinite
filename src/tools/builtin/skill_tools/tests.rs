//! Tests for the skill management tool module.

use std::sync::Arc;

use crate::skills::catalog::SkillCatalog;
use crate::skills::registry::SkillRegistry;
use crate::tools::tool::{ApprovalRequirement, Tool};

use super::{SkillInstallTool, SkillListTool, SkillRemoveTool, SkillSearchTool};

fn test_registry() -> Arc<std::sync::RwLock<SkillRegistry>> {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.keep();
    Arc::new(std::sync::RwLock::new(SkillRegistry::new(path)))
}

fn test_catalog() -> Arc<SkillCatalog> {
    Arc::new(SkillCatalog::with_url("http://127.0.0.1:1"))
}

/// Assert the common contract of a skill tool's static metadata.
fn assert_tool_schema(
    tool: &dyn Tool,
    expected_name: &str,
    expected_approval: ApprovalRequirement,
    expected_property_keys: &[&str],
) {
    assert_eq!(tool.name(), expected_name);
    assert_eq!(
        tool.requires_approval(&serde_json::json!({})),
        expected_approval
    );
    let schema = tool.parameters_schema();
    for key in expected_property_keys {
        assert!(
            schema["properties"].get(key).is_some(),
            "parameters_schema missing property '{key}'"
        );
    }
}

#[test]
fn test_skill_list_schema() {
    let tool = SkillListTool::new(test_registry());
    assert_tool_schema(&tool, "skill_list", ApprovalRequirement::Never, &[]);
}

#[test]
fn test_skill_search_schema() {
    let tool = SkillSearchTool::new(test_registry(), test_catalog());
    assert_tool_schema(
        &tool,
        "skill_search",
        ApprovalRequirement::Never,
        &["query"],
    );
}

#[test]
fn test_skill_install_schema() {
    let tool = SkillInstallTool::new(test_registry(), test_catalog());
    assert_tool_schema(
        &tool,
        "skill_install",
        ApprovalRequirement::UnlessAutoApproved,
        &["name", "url", "content"],
    );
}

#[test]
fn test_skill_remove_schema() {
    let tool = SkillRemoveTool::new(test_registry());
    assert_tool_schema(
        &tool,
        "skill_remove",
        ApprovalRequirement::Always,
        &["name"],
    );
}

#[test]
fn skill_remove_always_requires_approval_regardless_of_params() {
    let tool = SkillRemoveTool::new(test_registry());

    let test_cases = vec![
        ("no params", serde_json::json!({})),
        ("empty name", serde_json::json!({"name": ""})),
        (
            "deployment skill",
            serde_json::json!({"name": "deployment"}),
        ),
        ("custom skill", serde_json::json!({"name": "custom-skill"})),
        (
            "with extra fields",
            serde_json::json!({"name": "skill", "extra": "field"}),
        ),
    ];

    for (case_name, params) in test_cases {
        assert_eq!(
            tool.requires_approval(&params),
            ApprovalRequirement::Always,
            "skill_remove must always require approval for case: {}",
            case_name
        );
    }
}
