//! Tests for the skill management tool module.

use std::collections::BTreeSet;
use std::sync::Arc;

use rstest::{fixture, rstest};

use crate::context::JobContext;
use crate::skills::catalog::SkillCatalog;
use crate::skills::registry::SkillRegistry;
use crate::skills::{
    ActivationCriteria, LoadedSkill, LoadedSkillLocation, LoadedSkillParts, SkillManifest,
    SkillPackageKind, SkillSource, SkillTrust,
};
use crate::tools::tool::{ApprovalRequirement, NativeTool, Tool};

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

fn skill_markdown(name: &str) -> String {
    format!("---\nname: {name}\n---\n\n# {name}\n")
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

fn assert_schema_required_fields(tool: &dyn Tool, expected_required: &[&str]) {
    let schema = tool.parameters_schema();
    let actual = schema
        .get("required")
        .and_then(serde_json::Value::as_array)
        .map(|values| {
            values
                .iter()
                .filter_map(serde_json::Value::as_str)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    assert_eq!(actual, expected_required);
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
fn skill_install_schema_does_not_require_catalogue_name(test_registry: TestRegistryHandle) {
    let tool = SkillInstallTool::new(Arc::clone(&test_registry.registry), test_catalog());

    assert_schema_required_fields(&tool, &[]);
    assert!(
        NativeTool::parameters_schema(&tool)["properties"]["url"]["description"]
            .as_str()
            .is_some_and(|description| description.contains(".skill archive")),
        "install schema should describe .skill URL support"
    );
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
    assert_eq!(
        NativeTool::requires_approval(&tool, &params),
        ApprovalRequirement::Always,
    );
}

#[test]
fn skill_search_rejects_whitespace_only_query() {
    let err = SkillSearchTool::parse_search_query(&serde_json::json!({"query": "   \t  "}))
        .expect_err("whitespace-only query should be rejected");
    assert_eq!(
        err.to_string(),
        "Invalid parameters: query must not be empty"
    );
}

#[rstest]
#[case("search", true)]
#[case("discover skills", true)]
#[case("rust", true)]
#[case("python", false)]
fn skill_search_matches_query_checks_name_description_and_keywords(
    #[case] query: &str,
    #[case] expected: bool,
) {
    let skill = LoadedSkill::new(LoadedSkillParts {
        manifest: SkillManifest {
            name: "search-helper".to_string(),
            version: "1.0.0".to_string(),
            description: "Discover skills for Rust workflows".to_string(),
            activation: ActivationCriteria {
                keywords: vec!["skill-search".to_string(), "rust".to_string()],
                ..ActivationCriteria::default()
            },
            metadata: None,
        },
        prompt_content: String::new(),
        trust: SkillTrust::Trusted,
        source: SkillSource::Bundled(std::path::PathBuf::from("skills/search-helper")),
        location: LoadedSkillLocation::new(
            "search-helper",
            std::path::PathBuf::from("skills/search-helper"),
            std::path::PathBuf::from("SKILL.md"),
            SkillPackageKind::SingleFile,
        ),
        content_hash: "hash".to_string(),
        compiled_patterns: Vec::new(),
        lowercased_keywords: Vec::new(),
        lowercased_exclude_keywords: Vec::new(),
        lowercased_tags: Vec::new(),
    })
    .expect("test skill location should match manifest");

    assert_eq!(SkillSearchTool::matches_query(&skill, query), expected);
}

#[rstest]
#[tokio::test]
async fn skill_install_tool_installs_inline_content(test_registry: TestRegistryHandle) {
    let tool = SkillInstallTool::new(Arc::clone(&test_registry.registry), test_catalog());
    let output = NativeTool::execute(
        &tool,
        serde_json::json!({
            "content": skill_markdown("deploy-docs"),
        }),
        &JobContext::default(),
    )
    .await
    .expect("inline skill install should succeed");

    assert_eq!(output.result["name"], "deploy-docs");
    assert!(
        test_registry
            .registry
            .read()
            .expect("registry lock should be readable")
            .has("deploy-docs")
    );
}

#[rstest]
#[tokio::test]
async fn skill_install_tool_rejects_duplicate_inline_install(test_registry: TestRegistryHandle) {
    let tool = SkillInstallTool::new(Arc::clone(&test_registry.registry), test_catalog());
    let params = serde_json::json!({
        "content": skill_markdown("deploy-docs"),
    });

    NativeTool::execute(&tool, params.clone(), &JobContext::default())
        .await
        .expect("first inline install should succeed");

    let error = NativeTool::execute(&tool, params, &JobContext::default())
        .await
        .expect_err("duplicate inline install should fail");

    assert!(error.to_string().contains("already exists"));
}

#[rstest]
#[case::no_source(serde_json::json!({}))]
#[case::name_and_content(serde_json::json!({
    "name": "deploy-docs",
    "content": skill_markdown("deploy-docs"),
}))]
#[case::url_and_content(serde_json::json!({
    "url": "https://example.com/deploy-docs.skill",
    "content": skill_markdown("deploy-docs"),
}))]
#[case::url_and_name(serde_json::json!({
    "url": "https://example.com/deploy-docs.skill",
    "name": "deploy-docs",
}))]
#[case::content_url_and_name(serde_json::json!({
    "content": skill_markdown("deploy-docs"),
    "url": "https://example.com/deploy-docs.skill",
    "name": "deploy-docs",
}))]
#[tokio::test]
async fn skill_install_tool_rejects_ambiguous_sources(
    test_registry: TestRegistryHandle,
    #[case] params: serde_json::Value,
) {
    let tool = SkillInstallTool::new(Arc::clone(&test_registry.registry), test_catalog());

    let error = NativeTool::execute(&tool, params, &JobContext::default())
        .await
        .expect_err("ambiguous install source should fail");

    assert!(
        error
            .to_string()
            .contains("provide exactly one of 'content', 'url', or 'name'"),
        "unexpected error: {error}"
    );
}
