//! Tests for the skill management tool module.

use std::collections::BTreeSet;
use std::sync::Arc;

use rstest::{fixture, rstest};

use crate::skills::catalog::SkillCatalog;
use crate::skills::registry::SkillRegistry;
use crate::tools::tool::{ApprovalRequirement, Tool};

use super::{SkillInstallTool, SkillListTool, SkillRemoveTool, SkillSearchTool};

struct TestRegistryHandle {
    _dir: tempfile::TempDir,
    registry: Arc<std::sync::RwLock<SkillRegistry>>,
}

#[fixture]
fn test_registry() -> TestRegistryHandle {
    let dir = tempfile::tempdir().expect("tempdir creation failed");
    let path = dir.path().to_path_buf();
    TestRegistryHandle {
        _dir: dir,
        registry: Arc::new(std::sync::RwLock::new(SkillRegistry::new(path))),
    }
}

#[fixture]
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
    let actual_keys: BTreeSet<String> = schema["properties"]
        .as_object()
        .expect("parameters_schema should expose an object properties map")
        .keys()
        .cloned()
        .collect();
    let expected_keys: BTreeSet<String> = expected_property_keys
        .iter()
        .map(|key| (*key).to_string())
        .collect();
    assert_eq!(
        actual_keys, expected_keys,
        "unexpected parameters_schema keys"
    );
}

#[rstest]
fn test_skill_list_schema(test_registry: TestRegistryHandle) {
    let tool = SkillListTool::new(Arc::clone(&test_registry.registry));
    assert_tool_schema(
        &tool,
        "skill_list",
        ApprovalRequirement::Never,
        &["verbose"],
    );
}

#[rstest]
#[case(
    |r: &TestRegistryHandle, c: Arc<SkillCatalog>| -> Box<dyn Tool> {
        Box::new(SkillSearchTool::new(Arc::clone(&r.registry), c))
    },
    "skill_search",
    ApprovalRequirement::Never,
    &["query"] as &[&str],
)]
#[case(
    |r: &TestRegistryHandle, c: Arc<SkillCatalog>| -> Box<dyn Tool> {
        Box::new(SkillInstallTool::new(Arc::clone(&r.registry), c))
    },
    "skill_install",
    ApprovalRequirement::UnlessAutoApproved,
    &["name", "url", "content"] as &[&str],
)]
#[case(
    |r: &TestRegistryHandle, _c: Arc<SkillCatalog>| -> Box<dyn Tool> {
        Box::new(SkillRemoveTool::new(Arc::clone(&r.registry)))
    },
    "skill_remove",
    ApprovalRequirement::Always,
    &["name"] as &[&str],
)]
fn test_skill_tool_schema(
    test_registry: TestRegistryHandle,
    test_catalog: Arc<SkillCatalog>,
    #[case] make_tool: impl Fn(&TestRegistryHandle, Arc<SkillCatalog>) -> Box<dyn Tool>,
    #[case] expected_name: &str,
    #[case] expected_approval: ApprovalRequirement,
    #[case] expected_keys: &[&str],
) {
    let tool = make_tool(&test_registry, Arc::clone(&test_catalog));
    assert_tool_schema(&*tool, expected_name, expected_approval, expected_keys);
}

#[rstest]
#[case::no_params(serde_json::json!({}))]
#[case::empty_name(serde_json::json!({"name": ""}))]
#[case::deployment_skill(serde_json::json!({"name": "deployment"}))]
#[case::custom_skill(serde_json::json!({"name": "custom-skill"}))]
#[case::extra_fields(serde_json::json!({"name": "skill", "extra": "field"}))]
fn skill_remove_always_requires_approval_regardless_of_params(
    test_registry: TestRegistryHandle,
    #[case] params: serde_json::Value,
) {
    let tool = SkillRemoveTool::new(Arc::clone(&test_registry.registry));
    assert_eq!(tool.requires_approval(&params), ApprovalRequirement::Always,);
}
