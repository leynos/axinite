use std::fs;
use std::path::PathBuf;

use super::super::*;

#[tokio::test]
async fn test_has_and_find_by_name() {
    let dir = tempfile::tempdir().unwrap();
    let skill_dir = dir.path().join("my-skill");
    fs::create_dir(&skill_dir).unwrap();
    fs::write(
        skill_dir.join("SKILL.md"),
        "---\nname: my-skill\n---\n\nPrompt.\n",
    )
    .unwrap();

    let mut registry = SkillRegistry::new(dir.path().to_path_buf());
    registry.discover_all().await;

    assert!(registry.has("my-skill"));
    assert!(!registry.has("nonexistent"));
    assert!(registry.find_by_name("my-skill").is_some());
    assert!(registry.find_by_name("nonexistent").is_none());
}

#[tokio::test]
async fn test_retain_only_empty_is_noop() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(
        dir.path().join("SKILL.md"),
        "---\nname: keep-me\ndescription: test\nactivation:\n  keywords: [\"test\"]\n---\n\nKeep this skill.\n",
    )
    .unwrap();

    let mut registry = SkillRegistry::new(dir.path().to_path_buf());
    registry.discover_all().await;
    assert_eq!(registry.count(), 1);

    registry.retain_only(&[]);
    assert_eq!(
        registry.count(),
        1,
        "empty retain_only should keep all skills"
    );
}

#[test]
fn test_compute_hash_deterministic() {
    let h1 = compute_hash("hello world");
    let h2 = compute_hash("hello world");
    assert_eq!(h1, h2);
    assert!(h1.starts_with("sha256:"));
}

#[test]
fn test_compute_hash_different_content() {
    let h1 = compute_hash("hello");
    let h2 = compute_hash("world");
    assert_ne!(h1, h2);
}

#[tokio::test]
async fn test_installed_dir_uses_installed_trust() {
    let user_dir = tempfile::tempdir().unwrap();
    let inst_dir = tempfile::tempdir().unwrap();

    let skill_dir = inst_dir.path().join("registry-skill");
    fs::create_dir(&skill_dir).unwrap();
    fs::write(
        skill_dir.join("SKILL.md"),
        "---\nname: registry-skill\nversion: \"1.2.3\"\n---\n\nInstalled prompt.\n",
    )
    .unwrap();

    let mut registry = SkillRegistry::new(user_dir.path().to_path_buf())
        .with_installed_dir(inst_dir.path().to_path_buf());
    let loaded = registry.discover_all().await;

    assert_eq!(loaded, vec!["registry-skill"]);
    let skill = registry.find_by_name("registry-skill").unwrap();
    assert_eq!(
        skill.trust,
        SkillTrust::Installed,
        "installed_dir skills must be Installed"
    );
    assert_eq!(skill.manifest.version, "1.2.3");
}

#[test]
fn test_install_target_dir_prefers_installed_dir() {
    let user_dir = PathBuf::from("/tmp/user-skills");
    let inst_dir = PathBuf::from("/tmp/installed-skills");

    let registry = SkillRegistry::new(user_dir.clone()).with_installed_dir(inst_dir.clone());
    assert_eq!(registry.install_target_dir(), inst_dir.as_path());

    let registry_no_inst = SkillRegistry::new(user_dir.clone());
    assert_eq!(registry_no_inst.install_target_dir(), user_dir.as_path());
}

#[tokio::test]
async fn test_user_dir_stays_trusted_with_installed_dir() {
    let user_dir = tempfile::tempdir().unwrap();
    let inst_dir = tempfile::tempdir().unwrap();

    let skill_dir = user_dir.path().join("my-skill");
    fs::create_dir(&skill_dir).unwrap();
    fs::write(
        skill_dir.join("SKILL.md"),
        "---\nname: my-skill\n---\n\nUser prompt.\n",
    )
    .unwrap();

    let mut registry = SkillRegistry::new(user_dir.path().to_path_buf())
        .with_installed_dir(inst_dir.path().to_path_buf());
    registry.discover_all().await;

    let skill = registry.find_by_name("my-skill").unwrap();
    assert_eq!(skill.trust, SkillTrust::Trusted);
}
