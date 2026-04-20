use std::fs;
use std::path::PathBuf;

use super::super::*;

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

#[tokio::test]
async fn test_load_subdirectory_layout() {
    let dir = tempfile::tempdir().unwrap();
    let skill_dir = dir.path().join("test-skill");
    fs::create_dir(&skill_dir).unwrap();

    fs::write(
        skill_dir.join("SKILL.md"),
        "---\nname: test-skill\ndescription: A test skill\nactivation:\n  keywords: [\"test\"]\n---\n\nYou are a helpful test assistant.\n",
    )
    .unwrap();

    let mut registry = SkillRegistry::new(dir.path().to_path_buf());
    let loaded = registry.discover_all().await;

    assert_eq!(loaded, vec!["test-skill"]);
    assert_eq!(registry.count(), 1);

    let skill = &registry.skills()[0];
    assert_eq!(skill.trust, SkillTrust::Trusted);
    assert!(skill.prompt_content.contains("helpful test assistant"));
}

#[tokio::test]
async fn test_workspace_overrides_user() {
    let user_dir = tempfile::tempdir().unwrap();
    let ws_dir = tempfile::tempdir().unwrap();

    let user_skill = user_dir.path().join("my-skill");
    fs::create_dir(&user_skill).unwrap();
    fs::write(
        user_skill.join("SKILL.md"),
        "---\nname: my-skill\n---\n\nUser version.\n",
    )
    .unwrap();

    let ws_skill = ws_dir.path().join("my-skill");
    fs::create_dir(&ws_skill).unwrap();
    fs::write(
        ws_skill.join("SKILL.md"),
        "---\nname: my-skill\n---\n\nWorkspace version.\n",
    )
    .unwrap();

    let mut registry = SkillRegistry::new(user_dir.path().to_path_buf())
        .with_workspace_dir(ws_dir.path().to_path_buf());
    let loaded = registry.discover_all().await;

    assert_eq!(loaded, vec!["my-skill"]);
    assert_eq!(registry.count(), 1);
    assert!(registry.skills()[0].prompt_content.contains("Workspace"));
}

#[tokio::test]
async fn test_gating_failure_skips_skill() {
    let dir = tempfile::tempdir().unwrap();
    let skill_dir = dir.path().join("gated-skill");
    fs::create_dir(&skill_dir).unwrap();

    fs::write(
        skill_dir.join("SKILL.md"),
        "---\nname: gated-skill\nmetadata:\n  openclaw:\n    requires:\n      bins: [\"__nonexistent_bin__\"]\n---\n\nGated prompt.\n",
    )
    .unwrap();

    let mut registry = SkillRegistry::new(dir.path().to_path_buf());
    let loaded = registry.discover_all().await;
    assert!(loaded.is_empty());
}

#[cfg(unix)]
#[tokio::test]
async fn test_symlink_rejected() {
    let dir = tempfile::tempdir().unwrap();
    let real_dir = dir.path().join("real-skill");
    fs::create_dir(&real_dir).unwrap();
    fs::write(
        real_dir.join("SKILL.md"),
        "---\nname: real-skill\n---\n\nTest.\n",
    )
    .unwrap();

    let skills_dir = dir.path().join("skills");
    fs::create_dir(&skills_dir).unwrap();
    std::os::unix::fs::symlink(&real_dir, skills_dir.join("linked-skill")).unwrap();

    let mut registry = SkillRegistry::new(skills_dir);
    let loaded = registry.discover_all().await;
    assert!(loaded.is_empty());
}

#[tokio::test]
async fn test_file_size_limit() {
    let dir = tempfile::tempdir().unwrap();
    let skill_dir = dir.path().join("big-skill");
    fs::create_dir(&skill_dir).unwrap();

    let big_content = format!(
        "---\nname: big-skill\n---\n\n{}",
        "x".repeat((crate::skills::MAX_PROMPT_FILE_SIZE + 1) as usize)
    );
    fs::write(skill_dir.join("SKILL.md"), &big_content).unwrap();

    let mut registry = SkillRegistry::new(dir.path().to_path_buf());
    let loaded = registry.discover_all().await;
    assert!(loaded.is_empty());
}

#[tokio::test]
async fn test_invalid_skill_md_skipped() {
    let dir = tempfile::tempdir().unwrap();
    let skill_dir = dir.path().join("bad-skill");
    fs::create_dir(&skill_dir).unwrap();

    fs::write(skill_dir.join("SKILL.md"), "Just plain text").unwrap();

    let mut registry = SkillRegistry::new(dir.path().to_path_buf());
    let loaded = registry.discover_all().await;
    assert!(loaded.is_empty());
}

#[tokio::test]
async fn test_line_ending_normalization() {
    let dir = tempfile::tempdir().unwrap();
    let skill_dir = dir.path().join("crlf-skill");
    fs::create_dir(&skill_dir).unwrap();

    fs::write(
        skill_dir.join("SKILL.md"),
        "---\r\nname: crlf-skill\r\n---\r\n\r\nline1\r\nline2\r\n",
    )
    .unwrap();

    let mut registry = SkillRegistry::new(dir.path().to_path_buf());
    registry.discover_all().await;

    assert_eq!(registry.count(), 1);
    let skill = &registry.skills()[0];
    assert_eq!(skill.prompt_content, "line1\nline2\n");
}

#[tokio::test]
async fn test_token_budget_rejection() {
    let dir = tempfile::tempdir().unwrap();
    let skill_dir = dir.path().join("big-prompt");
    fs::create_dir(&skill_dir).unwrap();

    let big_prompt = "word ".repeat(4000);
    let content = format!(
        "---\nname: big-prompt\nactivation:\n  max_context_tokens: 100\n---\n\n{}",
        big_prompt
    );
    fs::write(skill_dir.join("SKILL.md"), &content).unwrap();

    let mut registry = SkillRegistry::new(dir.path().to_path_buf());
    let loaded = registry.discover_all().await;
    assert!(loaded.is_empty());
}

#[tokio::test]
async fn test_load_flat_layout() {
    let dir = tempfile::tempdir().unwrap();

    fs::write(
        dir.path().join("SKILL.md"),
        "---\nname: flat-skill\ndescription: A flat layout skill\nactivation:\n  keywords: [\"flat\"]\n---\n\nYou are a flat layout test skill.\n",
    )
    .unwrap();

    let mut registry = SkillRegistry::new(dir.path().to_path_buf());
    let loaded = registry.discover_all().await;

    assert_eq!(loaded, vec!["flat-skill"]);
    assert_eq!(registry.count(), 1);

    let skill = &registry.skills()[0];
    assert_eq!(skill.trust, SkillTrust::Trusted);
    assert!(skill.prompt_content.contains("flat layout test skill"));
}

#[tokio::test]
async fn test_mixed_flat_and_subdirectory_layout() {
    let dir = tempfile::tempdir().unwrap();

    fs::write(
        dir.path().join("SKILL.md"),
        "---\nname: flat-skill\n---\n\nFlat prompt.\n",
    )
    .unwrap();

    let sub_dir = dir.path().join("sub-skill");
    fs::create_dir(&sub_dir).unwrap();
    fs::write(
        sub_dir.join("SKILL.md"),
        "---\nname: sub-skill\n---\n\nSub prompt.\n",
    )
    .unwrap();

    let mut registry = SkillRegistry::new(dir.path().to_path_buf());
    let loaded = registry.discover_all().await;

    assert_eq!(registry.count(), 2);
    assert!(loaded.contains(&"flat-skill".to_string()));
    assert!(loaded.contains(&"sub-skill".to_string()));
}

#[tokio::test]
async fn test_lowercased_fields_populated() {
    let dir = tempfile::tempdir().unwrap();
    let skill_dir = dir.path().join("case-skill");
    fs::create_dir(&skill_dir).unwrap();

    fs::write(
        skill_dir.join("SKILL.md"),
        "---\nname: case-skill\nactivation:\n  keywords: [\"Write\", \"EDIT\"]\n  tags: [\"Email\", \"PROSE\"]\n---\n\nTest prompt.\n",
    )
    .unwrap();

    let mut registry = SkillRegistry::new(dir.path().to_path_buf());
    registry.discover_all().await;

    let skill = registry.find_by_name("case-skill").unwrap();
    assert_eq!(skill.lowercased_keywords, vec!["write", "edit"]);
    assert_eq!(skill.lowercased_tags, vec!["email", "prose"]);
}

#[tokio::test]
async fn test_reload_clears_and_rediscovers() {
    let dir = tempfile::tempdir().unwrap();
    let skill_dir = dir.path().join("persist-skill");
    fs::create_dir(&skill_dir).unwrap();
    fs::write(
        skill_dir.join("SKILL.md"),
        "---\nname: persist-skill\n---\n\nPrompt.\n",
    )
    .unwrap();

    let mut registry = SkillRegistry::new(dir.path().to_path_buf());
    registry.discover_all().await;
    assert_eq!(registry.count(), 1);

    let loaded = registry.reload().await;
    assert_eq!(loaded, vec!["persist-skill"]);
    assert_eq!(registry.count(), 1);
}
