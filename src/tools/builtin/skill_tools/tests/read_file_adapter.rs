//! Adapter tests for the `skill_read_file` builtin tool.

use std::sync::Arc;

use rstest::{fixture, rstest};
use rstest_bdd_macros::{given, scenario, then, when};

use crate::context::JobContext;
use crate::skills::registry::SkillRegistry;
#[cfg(target_os = "linux")]
use crate::skills::test_support::installed_bundle_fixture;
use crate::skills::{LoadedSkillLocation, SkillPackageKind};
use crate::tools::tool::NativeTool;

use super::super::SkillReadFileTool;

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

#[cfg(target_os = "linux")]
fn documented_bundle_entries() -> Vec<(&'static str, &'static [u8])> {
    vec![
        (
            "deploy-docs/SKILL.md",
            b"---\nname: deploy-docs\n---\n\n# deploy-docs\n",
        ),
        ("deploy-docs/references/usage.md", b"# Usage\n"),
        ("deploy-docs/references/nested/api.md", b"# API\n"),
        ("deploy-docs/assets/note.txt", b"asset notes\n"),
        (
            "deploy-docs/assets/logo.png",
            &[0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A],
        ),
    ]
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

#[rstest]
#[tokio::test]
async fn skill_read_file_tool_reads_bundle_reference(test_registry: TestRegistryHandle) {
    let bundle_dir = tempfile::tempdir().expect("bundle tempdir should be created");
    ambient_fs::create_dir_all(bundle_dir.path().join("references"))
        .expect("references dir should be created");
    ambient_fs::write(bundle_dir.path().join("SKILL.md"), "# Deploy docs\n")
        .expect("SKILL.md should be written");
    ambient_fs::write(bundle_dir.path().join("references/usage.md"), "# Usage\n")
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
#[case::entrypoint(
    "SKILL.md",
    "text/markdown",
    "---\nname: deploy-docs\n---\n\n# deploy-docs\n"
)]
#[case::reference("references/usage.md", "text/markdown", "# Usage\n")]
#[case::nested_reference("references/nested/api.md", "text/markdown", "# API\n")]
#[case::text_asset("assets/note.txt", "text/plain", "asset notes\n")]
#[tokio::test]
#[cfg(target_os = "linux")]
async fn test_skill_read_file_tool_after_install_returns_each_documented_entry(
    #[case] path: &str,
    #[case] mime_type: &str,
    #[case] content: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    let fixture = installed_bundle_fixture(&documented_bundle_entries()).await?;
    let _user_dir = fixture._user_dir;
    let _installed_dir = fixture._installed_dir;
    let registry = Arc::new(std::sync::RwLock::new(fixture.registry));
    let tool = SkillReadFileTool::new(registry);

    let output = NativeTool::execute(
        &tool,
        serde_json::json!({
            "skill": "deploy-docs",
            "path": path,
        }),
        &JobContext::default(),
    )
    .await
    .expect("skill_read_file should return installed text entry");

    assert_eq!(output.result["skill"], "deploy-docs");
    assert_eq!(output.result["path"], path);
    assert_eq!(output.result["mime_type"], mime_type);
    assert_eq!(output.result["content"], content);
    assert!(output.result.get("error").is_none());
    Ok(())
}

#[rstest]
#[tokio::test]
#[cfg(target_os = "linux")]
async fn test_skill_read_file_tool_after_install_returns_non_inline_for_png()
-> Result<(), Box<dyn std::error::Error>> {
    let fixture = installed_bundle_fixture(&documented_bundle_entries()).await?;
    let _user_dir = fixture._user_dir;
    let _installed_dir = fixture._installed_dir;
    let registry = Arc::new(std::sync::RwLock::new(fixture.registry));
    let tool = SkillReadFileTool::new(registry);

    let output = NativeTool::execute(
        &tool,
        serde_json::json!({
            "skill": "deploy-docs",
            "path": "assets/logo.png",
        }),
        &JobContext::default(),
    )
    .await
    .expect("skill_read_file should return non-inline payload");

    assert_eq!(output.result["skill"], "deploy-docs");
    assert_eq!(output.result["path"], "assets/logo.png");
    assert_eq!(output.result["error"]["code"], "non_inline_asset");
    assert_eq!(output.result["error"]["metadata"]["size"], 8);
    assert_eq!(output.result["error"]["metadata"]["mime_type"], "image/png");
    assert!(
        output.result["error"]["metadata"]["fetch_hint"]
            .as_str()
            .is_some_and(|hint| hint.contains("passive asset"))
    );
    Ok(())
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
    ambient_fs::create_dir_all(bundle_dir.path().join("references"))
        .expect("references dir should be created");
    ambient_fs::write(bundle_dir.path().join("SKILL.md"), "# Deploy docs\n")
        .expect("SKILL.md should be written");
    ambient_fs::write(bundle_dir.path().join("references/usage.md"), "# Usage\n")
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
