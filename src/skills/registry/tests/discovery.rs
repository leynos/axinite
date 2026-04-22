use std::fs;
use std::path::PathBuf;

use rstest::rstest;

use super::super::*;
use super::fixtures::{
    FreshRegistryFixture, fresh_registry_fixture, write_skill_flat, write_skill_subdir,
};

async fn assert_single_skill_loaded(
    registry: &mut SkillRegistry,
    expected_name: &str,
    expected_content_fragment: &str,
) {
    let loaded = registry.discover_all().await;
    assert_eq!(loaded, vec![expected_name]);
    assert_eq!(registry.count(), 1);
    let skill = &registry.skills()[0];
    assert_eq!(skill.trust, SkillTrust::Trusted);
    assert!(skill.prompt_content.contains(expected_content_fragment));
}

#[tokio::test]
async fn test_discover_empty_dir() {
    let dir = tempfile::tempdir().expect("create tempdir for test_discover_empty_dir");
    let mut registry = SkillRegistry::new(dir.path().to_path_buf());
    let loaded = registry.discover_all().await;
    assert!(loaded.is_empty());
}

#[tokio::test]
async fn test_discover_nonexistent_dir() {
    let mut registry = SkillRegistry::new(PathBuf::from("/nonexistent/skills"));
    let loaded = registry.discover_all().await;
    assert!(loaded.is_empty());
}

#[rstest]
#[tokio::test]
async fn test_load_subdirectory_layout(fresh_registry_fixture: FreshRegistryFixture) {
    let FreshRegistryFixture { dir, mut registry } = fresh_registry_fixture;
    write_skill_subdir(
        dir.path(),
        "test-skill",
        "---\nname: test-skill\ndescription: A test skill\nactivation:\n  keywords: [\"test\"]\n---\n\nYou are a helpful test assistant.\n",
    );
    assert_single_skill_loaded(&mut registry, "test-skill", "helpful test assistant").await;
}

#[tokio::test]
async fn test_workspace_overrides_user() {
    let user_dir = tempfile::tempdir().expect("temp dir should be created for test");
    let ws_dir = tempfile::tempdir().expect("temp dir should be created for test");

    let user_skill = user_dir.path().join("my-skill");
    fs::create_dir(&user_skill).expect("skill dir should be created for test");
    fs::write(
        user_skill.join("SKILL.md"),
        "---\nname: my-skill\n---\n\nUser version.\n",
    )
    .expect("SKILL.md should be written for test");

    let ws_skill = ws_dir.path().join("my-skill");
    fs::create_dir(&ws_skill).expect("skill dir should be created for test");
    fs::write(
        ws_skill.join("SKILL.md"),
        "---\nname: my-skill\n---\n\nWorkspace version.\n",
    )
    .expect("SKILL.md should be written for test");

    let mut registry = SkillRegistry::new(user_dir.path().to_path_buf())
        .with_workspace_dir(ws_dir.path().to_path_buf());
    let loaded = registry.discover_all().await;

    assert_eq!(loaded, vec!["my-skill"]);
    assert_eq!(registry.count(), 1);
    assert!(registry.skills()[0].prompt_content.contains("Workspace"));
}

#[rstest]
#[tokio::test]
async fn test_gating_failure_skips_skill(fresh_registry_fixture: FreshRegistryFixture) {
    let FreshRegistryFixture { dir, mut registry } = fresh_registry_fixture;
    write_skill_subdir(
        dir.path(),
        "gated-skill",
        "---\nname: gated-skill\nmetadata:\n  openclaw:\n    requires:\n      bins: [\"__nonexistent_bin__\"]\n---\n\nGated prompt.\n",
    );
    let loaded = registry.discover_all().await;
    assert!(loaded.is_empty());
}

#[cfg(unix)]
#[tokio::test]
async fn test_symlink_rejected() {
    let dir = tempfile::tempdir().expect("temp dir should be created for test");
    let real_dir = dir.path().join("real-skill");
    fs::create_dir(&real_dir).expect("skill dir should be created for test");
    fs::write(
        real_dir.join("SKILL.md"),
        "---\nname: real-skill\n---\n\nTest.\n",
    )
    .expect("SKILL.md should be written for test");

    let skills_dir = dir.path().join("skills");
    fs::create_dir(&skills_dir).expect("skills dir should be created for test");
    std::os::unix::fs::symlink(&real_dir, skills_dir.join("linked-skill"))
        .expect("symlink should be created for test");

    let mut registry = SkillRegistry::new(skills_dir);
    let loaded = registry.discover_all().await;
    assert!(loaded.is_empty());
}

#[rstest]
#[tokio::test]
async fn test_file_size_limit(fresh_registry_fixture: FreshRegistryFixture) {
    let FreshRegistryFixture { dir, mut registry } = fresh_registry_fixture;
    let big_content = format!(
        "---\nname: big-skill\n---\n\n{}",
        "x".repeat((crate::skills::MAX_PROMPT_FILE_SIZE + 1) as usize)
    );
    write_skill_subdir(dir.path(), "big-skill", &big_content);
    let loaded = registry.discover_all().await;
    assert!(loaded.is_empty());
}

#[rstest]
#[tokio::test]
async fn test_invalid_skill_md_skipped(fresh_registry_fixture: FreshRegistryFixture) {
    let FreshRegistryFixture { dir, mut registry } = fresh_registry_fixture;
    write_skill_subdir(dir.path(), "bad-skill", "Just plain text");
    let loaded = registry.discover_all().await;
    assert!(loaded.is_empty());
}

#[rstest]
#[tokio::test]
async fn test_line_ending_normalization(fresh_registry_fixture: FreshRegistryFixture) {
    let FreshRegistryFixture { dir, mut registry } = fresh_registry_fixture;
    write_skill_subdir(
        dir.path(),
        "crlf-skill",
        "---\r\nname: crlf-skill\r\n---\r\n\r\nline1\r\nline2\r\n",
    );
    registry.discover_all().await;

    assert_eq!(registry.count(), 1);
    let skill = &registry.skills()[0];
    assert_eq!(skill.prompt_content, "line1\nline2\n");
}

#[rstest]
#[tokio::test]
async fn test_token_budget_rejection(fresh_registry_fixture: FreshRegistryFixture) {
    let FreshRegistryFixture { dir, mut registry } = fresh_registry_fixture;
    let big_prompt = "word ".repeat(4000);
    let content = format!(
        "---\nname: big-prompt\nactivation:\n  max_context_tokens: 100\n---\n\n{}",
        big_prompt
    );
    write_skill_subdir(dir.path(), "big-prompt", &content);
    let loaded = registry.discover_all().await;
    assert!(loaded.is_empty());
}

#[rstest]
#[tokio::test]
async fn test_load_flat_layout(fresh_registry_fixture: FreshRegistryFixture) {
    let FreshRegistryFixture { dir, mut registry } = fresh_registry_fixture;
    write_skill_flat(
        dir.path(),
        "---\nname: flat-skill\ndescription: A flat layout skill\nactivation:\n  keywords: [\"flat\"]\n---\n\nYou are a flat layout test skill.\n",
    );
    assert_single_skill_loaded(&mut registry, "flat-skill", "flat layout test skill").await;
}

#[tokio::test]
async fn test_mixed_flat_and_subdirectory_layout() {
    let dir = tempfile::tempdir().expect("temp dir should be created for test");

    fs::write(
        dir.path().join("SKILL.md"),
        "---\nname: flat-skill\n---\n\nFlat prompt.\n",
    )
    .expect("flat SKILL.md should be written for test");

    let sub_dir = dir.path().join("sub-skill");
    fs::create_dir(&sub_dir).expect("skill dir should be created for test");
    fs::write(
        sub_dir.join("SKILL.md"),
        "---\nname: sub-skill\n---\n\nSub prompt.\n",
    )
    .expect("SKILL.md should be written for test");

    let mut registry = SkillRegistry::new(dir.path().to_path_buf());
    let loaded = registry.discover_all().await;

    assert_eq!(registry.count(), 2);
    assert!(loaded.contains(&"flat-skill".to_string()));
    assert!(loaded.contains(&"sub-skill".to_string()));
}

#[rstest]
#[tokio::test]
async fn test_lowercased_fields_populated(fresh_registry_fixture: FreshRegistryFixture) {
    let FreshRegistryFixture { dir, mut registry } = fresh_registry_fixture;
    write_skill_subdir(
        dir.path(),
        "case-skill",
        "---\nname: case-skill\nactivation:\n  keywords: [\"Write\", \"EDIT\"]\n  tags: [\"Email\", \"PROSE\"]\n---\n\nTest prompt.\n",
    );
    registry.discover_all().await;

    let skill = registry
        .find_by_name("case-skill")
        .expect("case-skill should be discovered for test");
    assert_eq!(skill.lowercased_keywords, vec!["write", "edit"]);
    assert_eq!(skill.lowercased_tags, vec!["email", "prose"]);
}

#[rstest]
#[tokio::test]
async fn test_reload_clears_and_rediscovers(fresh_registry_fixture: FreshRegistryFixture) {
    let FreshRegistryFixture { dir, mut registry } = fresh_registry_fixture;
    write_skill_subdir(
        dir.path(),
        "persist-skill",
        "---\nname: persist-skill\n---\n\nPrompt.\n",
    );
    registry.discover_all().await;
    assert_eq!(registry.count(), 1);

    let loaded = registry.reload().await;
    assert_eq!(loaded, vec!["persist-skill"]);
    assert_eq!(registry.count(), 1);
}
