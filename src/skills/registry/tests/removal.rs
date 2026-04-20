use std::fs;

use super::super::*;

#[tokio::test]
async fn test_remove_user_skill() {
    let dir = tempfile::tempdir().unwrap();
    let mut registry = SkillRegistry::new(dir.path().to_path_buf());

    let content = "---\nname: removable\n---\n\nPrompt.\n";
    registry.install_skill(content).await.unwrap();
    assert!(registry.has("removable"));

    registry.remove_skill("removable").await.unwrap();
    assert!(!registry.has("removable"));
    assert_eq!(registry.count(), 0);
}

#[tokio::test]
async fn test_remove_flat_layout_skill_preserves_siblings() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(
        dir.path().join("SKILL.md"),
        "---\nname: flat-skill\n---\n\nFlat prompt.\n",
    )
    .unwrap();

    let nested_dir = dir.path().join("nested-skill");
    fs::create_dir(&nested_dir).unwrap();
    fs::write(
        nested_dir.join("SKILL.md"),
        "---\nname: nested-skill\n---\n\nNested prompt.\n",
    )
    .unwrap();

    let mut registry = SkillRegistry::new(dir.path().to_path_buf());
    let loaded = registry.discover_all().await;
    assert_eq!(loaded.len(), 2, "fixture should load both sibling skills");

    registry
        .remove_skill("flat-skill")
        .await
        .expect("flat-layout skill should be removable");

    assert!(
        !dir.path().join("SKILL.md").exists(),
        "flat-layout SKILL.md should be removed"
    );
    assert!(
        nested_dir.join("SKILL.md").exists(),
        "removing a flat-layout skill must not delete sibling skill directories"
    );
    assert!(registry.has("nested-skill"));
    assert!(!registry.has("flat-skill"));
}

#[tokio::test]
async fn test_remove_workspace_skill_rejected() {
    let user_dir = tempfile::tempdir().unwrap();
    let ws_dir = tempfile::tempdir().unwrap();

    let ws_skill = ws_dir.path().join("ws-skill");
    fs::create_dir(&ws_skill).unwrap();
    fs::write(
        ws_skill.join("SKILL.md"),
        "---\nname: ws-skill\n---\n\nWorkspace prompt.\n",
    )
    .unwrap();

    let mut registry = SkillRegistry::new(user_dir.path().to_path_buf())
        .with_workspace_dir(ws_dir.path().to_path_buf());
    registry.discover_all().await;

    let result = registry.remove_skill("ws-skill").await;
    assert!(matches!(
        result,
        Err(SkillRegistryError::CannotRemove { .. })
    ));
}

#[tokio::test]
async fn test_remove_nonexistent_fails() {
    let dir = tempfile::tempdir().unwrap();
    let mut registry = SkillRegistry::new(dir.path().to_path_buf());

    let result = registry.remove_skill("nonexistent").await;
    assert!(matches!(result, Err(SkillRegistryError::NotFound(_))));
}
