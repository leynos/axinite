use std::fs;
use std::path::PathBuf;

use rstest::rstest;

use super::super::*;
use super::fixtures::{
    FreshRegistryFixture, fresh_registry_fixture, write_skill_flat, write_skill_subdir,
};
use crate::skills::SkillPackageKind;

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

#[cfg(unix)]
#[tokio::test]
async fn test_discover_unreadable_dir_returns_empty() {
    use std::os::unix::fs::PermissionsExt;

    struct PermissionsGuard<'a> {
        path: &'a std::path::Path,
    }

    impl Drop for PermissionsGuard<'_> {
        fn drop(&mut self) {
            std::fs::set_permissions(self.path, std::fs::Permissions::from_mode(0o755))
                .expect("permissions should be restored after test");
        }
    }

    let dir = tempfile::tempdir().expect("temp dir should be created for test");
    // Make the directory unreadable so read_dir will fail.
    std::fs::set_permissions(dir.path(), std::fs::Permissions::from_mode(0o000))
        .expect("permissions should be set for test");
    let _permissions_guard = PermissionsGuard { path: dir.path() };

    let mut registry = SkillRegistry::new(dir.path().to_path_buf());
    let loaded = registry.discover_all().await;

    assert!(
        loaded.is_empty(),
        "an unreadable skills directory should yield no skills, not a panic"
    );
}

#[derive(Debug)]
enum LayoutKind {
    Flat,
    Subdirectory,
}

#[rstest]
#[case::flat(
    LayoutKind::Flat,
    "flat-skill",
    "---\nname: flat-skill\ndescription: A flat layout skill\nactivation:\n  keywords: [\"flat\"]\n---\n\nYou are a flat layout test skill.\n",
    "flat layout test skill"
)]
#[case::subdirectory(
    LayoutKind::Subdirectory,
    "test-skill",
    "---\nname: test-skill\ndescription: A test skill\nactivation:\n  keywords: [\"test\"]\n---\n\nYou are a helpful test assistant.\n",
    "helpful test assistant"
)]
#[tokio::test]
async fn test_load_skill_layout(
    #[case] layout: LayoutKind,
    #[case] skill_name: &str,
    #[case] content: &str,
    #[case] content_fragment: &str,
    fresh_registry_fixture: FreshRegistryFixture,
) {
    let FreshRegistryFixture { dir, mut registry } = fresh_registry_fixture;
    let expected_root = match layout {
        LayoutKind::Flat => dir.path().to_path_buf(),
        LayoutKind::Subdirectory => dir.path().join(skill_name),
    };
    match layout {
        LayoutKind::Flat => write_skill_flat(dir.path(), content),
        LayoutKind::Subdirectory => write_skill_subdir(dir.path(), skill_name, content),
    }
    assert_single_skill_loaded(&mut registry, skill_name, content_fragment).await;
    let skill = registry
        .find_by_name(skill_name)
        .unwrap_or_else(|| panic!("{skill_name} should remain loaded"));
    assert_eq!(skill.skill_identifier(), skill_name);
    assert_eq!(skill.skill_root(), expected_root.as_path());
    assert_eq!(skill.skill_entrypoint(), std::path::Path::new("SKILL.md"));
    assert_eq!(skill.package_kind(), SkillPackageKind::SingleFile);
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
async fn test_bundle_layout_records_bundle_package_kind(
    fresh_registry_fixture: FreshRegistryFixture,
) {
    let FreshRegistryFixture { dir, mut registry } = fresh_registry_fixture;
    write_skill_subdir(
        dir.path(),
        "bundle-skill",
        "---\nname: bundle-skill\n---\n\nBundle prompt.\n",
    );
    std::fs::create_dir_all(dir.path().join("bundle-skill/references"))
        .expect("references dir should be created for test");
    std::fs::write(
        dir.path().join("bundle-skill/references/usage.md"),
        "# Usage\n",
    )
    .expect("reference file should be written for test");

    registry.discover_all().await;

    let skill = registry
        .find_by_name("bundle-skill")
        .expect("bundle-skill should be loaded");
    assert_eq!(skill.package_kind(), SkillPackageKind::Bundle);
    assert_eq!(
        skill.skill_root(),
        dir.path().join("bundle-skill").as_path()
    );
    assert_eq!(skill.skill_entrypoint(), std::path::Path::new("SKILL.md"));
}

#[rstest]
#[tokio::test]
async fn test_bundle_layout_records_bundle_package_kind_via_assets_dir(
    fresh_registry_fixture: FreshRegistryFixture,
) {
    let FreshRegistryFixture { dir, mut registry } = fresh_registry_fixture;
    write_skill_subdir(
        dir.path(),
        "bundle-skill",
        "---\nname: bundle-skill\n---\n\nBundle prompt.\n",
    );
    std::fs::create_dir_all(dir.path().join("bundle-skill/assets/images"))
        .expect("assets dir should be created for test");
    std::fs::write(
        dir.path().join("bundle-skill/assets/images/logo.png"),
        b"\x89PNG\r\n\x1a\n",
    )
    .expect("asset file should be written for test");

    registry.discover_all().await;

    let skill = registry
        .find_by_name("bundle-skill")
        .expect("bundle-skill should be loaded");
    assert_eq!(skill.package_kind(), SkillPackageKind::Bundle);
    assert_eq!(
        skill.skill_root(),
        dir.path().join("bundle-skill").as_path()
    );
    assert_eq!(skill.skill_entrypoint(), std::path::Path::new("SKILL.md"));
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
    let skill = registry
        .find_by_name("persist-skill")
        .expect("persist-skill should be rediscovered");
    assert_eq!(
        skill.skill_root(),
        dir.path().join("persist-skill").as_path()
    );
    assert_eq!(skill.skill_entrypoint(), std::path::Path::new("SKILL.md"));
    assert_eq!(skill.package_kind(), SkillPackageKind::SingleFile);
}
