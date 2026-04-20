use std::fs;
use std::path::PathBuf;

use rstest::rstest;

use super::*;

mod fixtures;

use fixtures::{
    BundleInstallFixture, build_bundle_archive, bundle_install_fixture, skill_markdown,
};

#[tokio::test]
async fn test_discover_empty_dir() {
    let dir = tempfile::tempdir().unwrap();
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
async fn test_install_skill_from_content() {
    let dir = tempfile::tempdir().unwrap();
    let mut registry = SkillRegistry::new(dir.path().to_path_buf());

    let content =
        "---\nname: test-install\ndescription: Installed skill\n---\n\nInstalled prompt.\n";
    let name = registry.install_skill(content).await.unwrap();

    assert_eq!(name, "test-install");
    assert!(registry.has("test-install"));
    assert_eq!(registry.count(), 1);

    let skill_path = dir.path().join("test-install").join("SKILL.md");
    assert!(skill_path.exists());
}

#[rstest]
#[tokio::test]
async fn test_install_bundle_from_downloaded_bytes_preserves_files(
    bundle_install_fixture: BundleInstallFixture,
) {
    let BundleInstallFixture {
        user_dir: _user_dir,
        installed_dir,
        mut registry,
    } = bundle_install_fixture;

    let archive = build_bundle_archive(&[
        (
            "deploy-docs/SKILL.md",
            skill_markdown("deploy-docs").as_bytes(),
        ),
        ("deploy-docs/references/usage.md", b"# Usage\n"),
        ("deploy-docs/assets/logo.txt", b"logo"),
    ]);

    let prepared = SkillRegistry::prepare_install_to_disk(
        registry.install_target_dir(),
        SkillInstallPayload::DownloadedBytes(archive),
    )
    .await
    .expect("bundle install should prepare successfully");

    registry
        .commit_install(&prepared)
        .expect("prepared bundle should commit successfully");

    let installed_root = installed_dir.path().join("deploy-docs");
    assert!(installed_root.join("SKILL.md").exists());
    assert!(installed_root.join("references/usage.md").exists());
    assert!(installed_root.join("assets/logo.txt").exists());
    assert!(registry.has("deploy-docs"));
}

#[tokio::test]
async fn test_install_duplicate_rejected() {
    let dir = tempfile::tempdir().unwrap();
    let mut registry = SkillRegistry::new(dir.path().to_path_buf());

    let content = "---\nname: dup-skill\n---\n\nPrompt.\n";
    registry.install_skill(content).await.unwrap();

    let result = registry.install_skill(content).await;
    assert!(matches!(
        result,
        Err(SkillRegistryError::AlreadyExists { .. })
    ));
}

#[rstest]
#[tokio::test]
async fn test_cleanup_prepared_install_removes_staged_bundle_on_commit_failure(
    bundle_install_fixture: BundleInstallFixture,
) {
    let BundleInstallFixture {
        user_dir: _user_dir,
        installed_dir: _installed_dir,
        mut registry,
    } = bundle_install_fixture;

    let archive = build_bundle_archive(&[(
        "deploy-docs/SKILL.md",
        skill_markdown("deploy-docs").as_bytes(),
    )]);

    let first = SkillRegistry::prepare_install_to_disk(
        registry.install_target_dir(),
        SkillInstallPayload::DownloadedBytes(archive.clone()),
    )
    .await
    .expect("first bundle should prepare");
    registry
        .commit_install(&first)
        .expect("first bundle should commit");

    let second = SkillRegistry::prepare_install_to_disk(
        registry.install_target_dir(),
        SkillInstallPayload::DownloadedBytes(archive),
    )
    .await
    .expect("second bundle should still stage");

    let staged_dir = second.staged_dir.clone();
    let error = registry
        .commit_install(&second)
        .expect_err("duplicate bundle should fail commit");
    assert!(matches!(error, SkillRegistryError::AlreadyExists { .. }));
    assert!(
        staged_dir.exists(),
        "failed commit should leave staged files for cleanup"
    );

    SkillRegistry::cleanup_prepared_install(&second)
        .await
        .expect("cleanup should remove staged directory");
    assert!(
        !staged_dir.exists(),
        "cleanup should remove staged directory"
    );
}

#[rstest]
#[tokio::test]
async fn test_remove_bundle_skill_allows_reinstall(bundle_install_fixture: BundleInstallFixture) {
    let BundleInstallFixture {
        installed_dir,
        mut registry,
        ..
    } = bundle_install_fixture;

    let archive = build_bundle_archive(&[
        (
            "deploy-docs/SKILL.md",
            skill_markdown("deploy-docs").as_bytes(),
        ),
        ("deploy-docs/references/usage.md", b"# Usage\n"),
        ("deploy-docs/assets/logo.txt", b"logo"),
    ]);

    let prepared = SkillRegistry::prepare_install_to_disk(
        registry.install_target_dir(),
        SkillInstallPayload::DownloadedBytes(archive.clone()),
    )
    .await
    .expect("bundle install should prepare successfully");
    registry
        .commit_install(&prepared)
        .expect("prepared bundle should commit successfully");

    let installed_root = installed_dir.path().join("deploy-docs");
    assert!(
        installed_root.join("references/usage.md").exists(),
        "bundle install should materialize ancillary files"
    );

    registry
        .remove_skill("deploy-docs")
        .await
        .expect("bundle install should be removable");
    assert!(
        !installed_root.exists(),
        "bundle uninstall should remove the full installed tree"
    );

    let prepared = SkillRegistry::prepare_install_to_disk(
        registry.install_target_dir(),
        SkillInstallPayload::DownloadedBytes(archive),
    )
    .await
    .expect("bundle reinstall should prepare successfully");
    registry
        .commit_install(&prepared)
        .expect("bundle reinstall should commit successfully");

    assert!(installed_root.join("SKILL.md").exists());
    assert!(installed_root.join("references/usage.md").exists());
    assert!(installed_root.join("assets/logo.txt").exists());
}

#[rstest]
#[tokio::test]
async fn test_prepare_install_cleans_staged_dir_when_validation_fails(
    bundle_install_fixture: BundleInstallFixture,
) {
    let BundleInstallFixture {
        installed_dir,
        registry,
        ..
    } = bundle_install_fixture;

    let archive = build_bundle_archive(&[("deploy-docs/SKILL.md", b"not valid skill markdown")]);

    let prepare_result = SkillRegistry::prepare_install_to_disk(
        registry.install_target_dir(),
        SkillInstallPayload::DownloadedBytes(archive),
    )
    .await;
    let error = match prepare_result {
        Ok(_) => panic!("invalid staged skill should fail validation"),
        Err(error) => error,
    };
    assert!(
        matches!(error, SkillRegistryError::ParseError { .. }),
        "expected parse error for invalid staged skill, got {error:?}"
    );

    let install_root_entries = std::fs::read_dir(installed_dir.path())
        .expect("installed dir should remain readable after failed prepare");
    let leaked_staged_dirs = install_root_entries
        .filter_map(Result::ok)
        .map(|entry| entry.file_name())
        .map(|name| name.to_string_lossy().into_owned())
        .filter(|name| name.starts_with(".skill-install-"))
        .collect::<Vec<_>>();
    assert!(
        leaked_staged_dirs.is_empty(),
        "failed staged validation should not leak temp dirs: {leaked_staged_dirs:?}"
    );
}

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
