//! Behaviour tests for model-facing active skill bundle metadata.

use std::path::PathBuf;

use rstest::fixture;
use rstest_bdd_macros::{given, scenario, then, when};

use super::*;
use crate::skills::{
    ActivationCriteria, LoadedSkill, LoadedSkillLocation, LoadedSkillParts, SkillManifest,
    SkillPackageKind, SkillSource, SkillTrust,
};

#[derive(Default)]
struct SkillContextWorld {
    active_skill: Option<LoadedSkill>,
    rendered_context: Option<String>,
    filesystem_root: Option<PathBuf>,
}

#[fixture]
fn skill_context_world() -> SkillContextWorld {
    SkillContextWorld::default()
}

fn make_loaded_bundle_skill(skill: &str, filesystem_root: PathBuf) -> LoadedSkill {
    LoadedSkill::new(LoadedSkillParts {
        manifest: SkillManifest {
            name: skill.to_string(),
            version: "1.0.0".to_string(),
            description: "Deploy documentation workflow".to_string(),
            activation: ActivationCriteria {
                keywords: vec!["deploy".to_string(), "docs".to_string()],
                max_context_tokens: 1000,
                ..ActivationCriteria::default()
            },
            metadata: None,
        },
        prompt_content: "Use references/usage.md for deployment details.".to_string(),
        trust: SkillTrust::Installed,
        source: SkillSource::User(filesystem_root.clone()),
        location: LoadedSkillLocation::new(
            skill,
            filesystem_root,
            PathBuf::from("SKILL.md"),
            SkillPackageKind::Bundle,
        )
        .expect("test entrypoint is bundle-relative"),
        content_hash: format!("sha256:{skill}"),
        compiled_patterns: Vec::new(),
        lowercased_keywords: vec!["deploy".to_string(), "docs".to_string()],
        lowercased_exclude_keywords: Vec::new(),
        lowercased_tags: Vec::new(),
    })
    .expect("BDD skill location should match manifest")
}

#[given("an installed bundled skill with supporting files")]
fn installed_bundled_skill(skill_context_world: &mut SkillContextWorld) {
    let filesystem_root = PathBuf::from("/tmp/axinite-test-installed/deploy-docs");
    skill_context_world.filesystem_root = Some(filesystem_root.clone());
    skill_context_world.active_skill =
        Some(make_loaded_bundle_skill("deploy-docs", filesystem_root));
}

#[when("the skill is selected for an agent turn")]
fn selected_for_agent_turn(skill_context_world: &mut SkillContextWorld) {
    let agent = make_test_agent();
    let skill = skill_context_world
        .active_skill
        .clone()
        .expect("Given step should install an active skill");
    let rendered = agent
        .build_skill_context_block(&[skill])
        .expect("installed bundle skill should produce context");
    skill_context_world.rendered_context = Some(rendered);
}

#[then("the active skill context names the skill identifier")]
fn context_names_skill_identifier(skill_context_world: &SkillContextWorld) {
    let rendered = rendered_context(skill_context_world);
    assert!(rendered.contains("skill=\"deploy-docs\""));
    assert!(rendered.contains("root=\".\""));
    assert!(rendered.contains("package=\"bundle\""));
}

#[then("the active skill context names SKILL.md as the entrypoint")]
fn context_names_entrypoint(skill_context_world: &SkillContextWorld) {
    let rendered = rendered_context(skill_context_world);
    assert!(rendered.contains("entry=\"SKILL.md\""));
}

#[then("the active skill context does not expose the filesystem root")]
fn context_hides_filesystem_root(skill_context_world: &SkillContextWorld) {
    let rendered = rendered_context(skill_context_world);
    let filesystem_root = skill_context_world
        .filesystem_root
        .as_ref()
        .expect("Given step should record the runtime root");
    assert!(
        !rendered.contains(&filesystem_root.to_string_lossy().to_string()),
        "active skill context must not expose the private runtime root"
    );
}

fn rendered_context(skill_context_world: &SkillContextWorld) -> &str {
    skill_context_world
        .rendered_context
        .as_deref()
        .expect("When step should render active skill context")
}

#[scenario(
    path = "src/agent/dispatcher/tests/features/active_skill_context.feature",
    name = "Selected bundle skill exposes stable bundle-relative metadata"
)]
fn selected_bundle_skill_exposes_stable_bundle_relative_metadata(
    skill_context_world: SkillContextWorld,
) {
    assert!(skill_context_world.rendered_context.is_some());
}
