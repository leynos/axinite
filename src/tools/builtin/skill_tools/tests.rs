//! Tests for the skill management tool module.

use std::collections::BTreeSet;
use std::sync::Arc;

use rstest::{fixture, rstest};
use rstest_bdd_macros::{given, scenario, then, when};

use crate::context::JobContext;
use crate::skills::catalog::SkillCatalog;
use crate::skills::registry::SkillRegistry;
use crate::skills::{LoadedSkillLocation, SkillPackageKind, SkillSource};
use crate::tools::tool::{ApprovalRequirement, NativeTool, Tool};

use super::{SkillInstallTool, SkillListTool, SkillReadFileTool, SkillRemoveTool, SkillSearchTool};

struct TestRegistryHandle {
    _dir: tempfile::TempDir,
    registry: Arc<std::sync::RwLock<SkillRegistry>>,
}

#[derive(Default)]
struct SkillReadFileWorld {
    bundle_dir: Option<tempfile::TempDir>,
    registry: Option<Arc<std::sync::RwLock<SkillRegistry>>>,
    output: Option<serde_json::Value>,
}

#[fixture]
fn skill_read_file_world() -> SkillReadFileWorld {
    SkillReadFileWorld::default()
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

fn insert_deploy_docs_bundle(
    registry: &Arc<std::sync::RwLock<SkillRegistry>>,
    root: &std::path::Path,
) {
    let location = LoadedSkillLocation::new(
        "deploy-docs",
        root,
        std::path::PathBuf::from("SKILL.md"),
        SkillPackageKind::Bundle,
    )
    .expect("bundle location should be valid");
    let skill = crate::skills::test_support::TestSkillBuilder::new("deploy-docs")
        .location(location)
        .build();
    registry
        .write()
        .expect("registry lock should be writable")
        .commit_loaded_skill("deploy-docs", skill)
        .expect("skill should be inserted");
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
#[case(
    |r: &TestRegistryHandle, _c: Arc<SkillCatalog>| -> Box<dyn Tool> {
        Box::new(SkillReadFileTool::new(Arc::clone(&r.registry)))
    },
    "skill_read_file",
    ApprovalRequirement::Never,
    &["path", "skill"] as &[&str],
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
fn skill_read_file_schema_is_strict(test_registry: TestRegistryHandle) {
    let tool = SkillReadFileTool::new(Arc::clone(&test_registry.registry));
    let schema = NativeTool::parameters_schema(&tool);

    assert_schema_required_fields(&tool, &["skill", "path"]);
    assert_eq!(schema["additionalProperties"], false);
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
    let skill = crate::skills::test_support::TestSkillBuilder::new("search-helper")
        .description("Discover skills for Rust workflows")
        .source(SkillSource::Bundled(std::path::PathBuf::from(
            "skills/search-helper",
        )))
        .location(
            LoadedSkillLocation::new(
                "search-helper",
                std::path::PathBuf::from("skills/search-helper"),
                std::path::PathBuf::from("SKILL.md"),
                SkillPackageKind::SingleFile,
            )
            .expect("test entrypoint is bundle-relative"),
        )
        .keywords(&["skill-search", "rust"])
        .prompt_content("")
        .content_hash("hash")
        .build();

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
async fn skill_read_file_tool_reads_bundle_reference(test_registry: TestRegistryHandle) {
    let bundle_dir = tempfile::tempdir().expect("bundle tempdir should be created");
    std::fs::create_dir_all(bundle_dir.path().join("references"))
        .expect("references dir should be created");
    std::fs::write(bundle_dir.path().join("SKILL.md"), "# Deploy docs\n")
        .expect("SKILL.md should be written");
    std::fs::write(bundle_dir.path().join("references/usage.md"), "# Usage\n")
        .expect("reference should be written");
    insert_deploy_docs_bundle(&test_registry.registry, bundle_dir.path());

    let tool = SkillReadFileTool::new(Arc::clone(&test_registry.registry));
    let output = NativeTool::execute(
        &tool,
        serde_json::json!({
            "skill": "deploy-docs",
            "path": "references/usage.md",
        }),
        &JobContext::default(),
    )
    .await
    .expect("skill_read_file should succeed");

    assert_eq!(output.result["skill"], "deploy-docs");
    assert_eq!(output.result["path"], "references/usage.md");
    assert_eq!(output.result["content"], "# Usage\n");
    assert!(output.result.get("error").is_none());
}

#[rstest]
#[tokio::test]
async fn skill_read_file_tool_reports_unknown_skill(test_registry: TestRegistryHandle) {
    let tool = SkillReadFileTool::new(Arc::clone(&test_registry.registry));

    let output = NativeTool::execute(
        &tool,
        serde_json::json!({
            "skill": "missing",
            "path": "SKILL.md",
        }),
        &JobContext::default(),
    )
    .await
    .expect("unknown skill should be a structured tool result");

    assert_eq!(output.result["skill"], "missing");
    assert_eq!(output.result["path"], "SKILL.md");
    assert_eq!(output.result["error"]["code"], "unknown_skill");
}

#[given("a loaded skill bundle with a referenced usage file")]
fn bdd_loaded_skill_bundle(skill_read_file_world: &mut SkillReadFileWorld) {
    let bundle_dir = tempfile::tempdir().expect("bundle tempdir should be created");
    std::fs::create_dir_all(bundle_dir.path().join("references"))
        .expect("references dir should be created");
    std::fs::write(bundle_dir.path().join("SKILL.md"), "# Deploy docs\n")
        .expect("SKILL.md should be written");
    std::fs::write(bundle_dir.path().join("references/usage.md"), "# Usage\n")
        .expect("reference should be written");

    let registry = Arc::new(std::sync::RwLock::new(SkillRegistry::new(
        bundle_dir.path().join("unused-user-dir"),
    )));
    insert_deploy_docs_bundle(&registry, bundle_dir.path());

    skill_read_file_world.bundle_dir = Some(bundle_dir);
    skill_read_file_world.registry = Some(registry);
}

#[when("the model calls skill_read_file for the usage file")]
fn bdd_model_reads_usage_file(skill_read_file_world: &mut SkillReadFileWorld) {
    execute_bdd_read(
        skill_read_file_world,
        serde_json::json!({
            "skill": "deploy-docs",
            "path": "references/usage.md",
        }),
    );
}

#[when("the model calls skill_read_file with a traversal path")]
fn bdd_model_reads_traversal_path(skill_read_file_world: &mut SkillReadFileWorld) {
    execute_bdd_read(
        skill_read_file_world,
        serde_json::json!({
            "skill": "deploy-docs",
            "path": "../secrets.txt",
        }),
    );
}

#[then("the tool returns the referenced text without a host filesystem path")]
fn bdd_tool_returns_reference_text(skill_read_file_world: &SkillReadFileWorld) {
    let output = skill_read_file_world
        .output
        .as_ref()
        .expect("When step should execute tool");
    assert_eq!(output["skill"], "deploy-docs");
    assert_eq!(output["path"], "references/usage.md");
    assert_eq!(output["content"], "# Usage\n");

    let root = skill_read_file_world
        .bundle_dir
        .as_ref()
        .expect("Given step should create bundle")
        .path()
        .to_string_lossy();
    assert!(!output.to_string().contains(root.as_ref()));
}

#[then("the tool returns a skill-scoped path_not_readable error")]
fn bdd_tool_returns_path_not_readable(skill_read_file_world: &SkillReadFileWorld) {
    let output = skill_read_file_world
        .output
        .as_ref()
        .expect("When step should execute tool");
    assert_eq!(output["skill"], "deploy-docs");
    assert_eq!(output["path"], "../secrets.txt");
    assert_eq!(output["error"]["code"], "path_not_readable");
}

fn execute_bdd_read(skill_read_file_world: &mut SkillReadFileWorld, params: serde_json::Value) {
    let registry = Arc::clone(
        skill_read_file_world
            .registry
            .as_ref()
            .expect("Given step should create registry"),
    );
    let tool = SkillReadFileTool::new(registry);
    let runtime = tokio::runtime::Runtime::new().expect("tokio runtime should start");
    let output = runtime
        .block_on(NativeTool::execute(&tool, params, &JobContext::default()))
        .expect("skill_read_file should return a tool output");
    skill_read_file_world.output = Some(output.result);
}

#[scenario(
    path = "src/tools/builtin/skill_tools/features/skill_read_file.feature",
    name = "A model reads a referenced bundled text file"
)]
fn bdd_model_reads_referenced_bundled_text_file(skill_read_file_world: SkillReadFileWorld) {
    assert!(skill_read_file_world.output.is_some());
}

#[scenario(
    path = "src/tools/builtin/skill_tools/features/skill_read_file.feature",
    name = "A model is denied raw filesystem traversal"
)]
fn bdd_model_is_denied_raw_filesystem_traversal(skill_read_file_world: SkillReadFileWorld) {
    assert!(skill_read_file_world.output.is_some());
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
