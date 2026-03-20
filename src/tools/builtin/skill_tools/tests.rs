//! Tests for the skill management tool module.

use std::sync::Arc;

use rstest::rstest;

use crate::skills::catalog::SkillCatalog;
use crate::skills::registry::SkillRegistry;
use crate::tools::tool::{ApprovalRequirement, Tool};

use super::{SkillInstallTool, SkillListTool, SkillRemoveTool, SkillSearchTool};

struct TestRegistryHandle {
    _dir: tempfile::TempDir,
    registry: Arc<std::sync::RwLock<SkillRegistry>>,
}

fn test_registry() -> TestRegistryHandle {
    let dir = tempfile::tempdir().expect("tempdir creation failed");
    let path = dir.path().to_path_buf();
    TestRegistryHandle {
        _dir: dir,
        registry: Arc::new(std::sync::RwLock::new(SkillRegistry::new(path))),
    }
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
    let registry = test_registry();
    let tool = SkillListTool::new(Arc::clone(&registry.registry));
    assert_tool_schema(&tool, "skill_list", ApprovalRequirement::Never, &[]);
}

#[rstest]
#[case(
    |r: &TestRegistryHandle| -> Box<dyn Tool> {
        Box::new(SkillSearchTool::new(Arc::clone(&r.registry), test_catalog()))
    },
    "skill_search",
    ApprovalRequirement::Never,
    &["query"] as &[&str],
)]
#[case(
    |r: &TestRegistryHandle| -> Box<dyn Tool> {
        Box::new(SkillInstallTool::new(Arc::clone(&r.registry), test_catalog()))
    },
    "skill_install",
    ApprovalRequirement::UnlessAutoApproved,
    &["name", "url", "content"] as &[&str],
)]
#[case(
    |r: &TestRegistryHandle| -> Box<dyn Tool> {
        Box::new(SkillRemoveTool::new(Arc::clone(&r.registry)))
    },
    "skill_remove",
    ApprovalRequirement::Always,
    &["name"] as &[&str],
)]
fn test_skill_tool_schema(
    #[case] make_tool: impl Fn(&TestRegistryHandle) -> Box<dyn Tool>,
    #[case] expected_name: &str,
    #[case] expected_approval: ApprovalRequirement,
    #[case] expected_keys: &[&str],
) {
    let registry = test_registry();
    let tool = make_tool(&registry);
    assert_tool_schema(&*tool, expected_name, expected_approval, expected_keys);
}

#[rstest]
#[case::no_params(serde_json::json!({}))]
#[case::empty_name(serde_json::json!({"name": ""}))]
#[case::deployment_skill(serde_json::json!({"name": "deployment"}))]
#[case::custom_skill(serde_json::json!({"name": "custom-skill"}))]
#[case::extra_fields(serde_json::json!({"name": "skill", "extra": "field"}))]
fn skill_remove_always_requires_approval_regardless_of_params(#[case] params: serde_json::Value) {
    let registry = test_registry();
    let tool = SkillRemoveTool::new(Arc::clone(&registry.registry));
    assert_eq!(tool.requires_approval(&params), ApprovalRequirement::Always,);
}
