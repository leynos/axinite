use rstest::rstest;

use super::super::*;
use super::fixtures::{
    BundleInstallFixture, build_bundle_archive, bundle_install_fixture, skill_markdown,
};

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
#[case::downloaded_bytes(
    |b| SkillInstallPayload::DownloadedBytes(b),
    "downloaded bytes install should prepare successfully",
    "prepared downloaded bytes should commit successfully",
)]
#[case::archive_bytes(
    |b| SkillInstallPayload::ArchiveBytes(b),
    "uploaded archive install should prepare successfully",
    "prepared uploaded archive should commit successfully",
)]
#[tokio::test]
async fn test_archive_payload_preserves_files(
    bundle_install_fixture: BundleInstallFixture,
    #[case] make_payload: impl FnOnce(Vec<u8>) -> SkillInstallPayload,
    #[case] prepare_msg: &'static str,
    #[case] commit_msg: &'static str,
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
        make_payload(archive),
    )
    .await
    .expect(prepare_msg);

    registry.commit_install(prepared).expect(commit_msg);

    let installed_root = installed_dir.path().join("deploy-docs");
    assert!(installed_root.join("SKILL.md").exists());
    assert!(installed_root.join("references/usage.md").exists());
    assert!(installed_root.join("assets/logo.txt").exists());
    assert!(registry.has("deploy-docs"));
}

#[rstest]
#[tokio::test]
async fn test_uploaded_archive_bytes_reject_plain_markdown(
    bundle_install_fixture: BundleInstallFixture,
) {
    let BundleInstallFixture { registry, .. } = bundle_install_fixture;

    let error = SkillRegistry::prepare_install_to_disk(
        registry.install_target_dir(),
        SkillInstallPayload::ArchiveBytes(skill_markdown("deploy-docs").into_bytes()),
    )
    .await
    .expect_err("uploaded archive bytes should not fall back to plain markdown");

    assert!(
        error.to_string().contains("invalid_skill_bundle"),
        "expected explicit bundle error, got {error}"
    );
}

#[rstest]
#[tokio::test]
async fn test_downloaded_bytes_accept_plain_markdown(bundle_install_fixture: BundleInstallFixture) {
    let BundleInstallFixture {
        installed_dir,
        mut registry,
        ..
    } = bundle_install_fixture;

    let prepared = SkillRegistry::prepare_install_to_disk(
        registry.install_target_dir(),
        SkillInstallPayload::DownloadedBytes(skill_markdown("deploy-docs").into_bytes()),
    )
    .await
    .expect("downloaded markdown should prepare successfully");

    registry
        .commit_install(prepared)
        .expect("downloaded markdown should commit successfully");

    assert!(installed_dir.path().join("deploy-docs/SKILL.md").exists());
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
        .commit_install(first)
        .expect("first bundle should commit");

    let second = SkillRegistry::prepare_install_to_disk(
        registry.install_target_dir(),
        SkillInstallPayload::DownloadedBytes(archive),
    )
    .await
    .expect("second bundle should still stage");

    let staged_dir = second.staged_dir.clone();
    let commit_failure = registry
        .commit_install(second)
        .expect_err("duplicate bundle should fail commit");
    let (error, prepared) = commit_failure.into_parts();
    assert!(matches!(error, SkillRegistryError::AlreadyExists { .. }));
    assert!(
        staged_dir.exists(),
        "failed commit should leave staged files for cleanup"
    );

    SkillRegistry::cleanup_prepared_install(&prepared)
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
        .commit_install(prepared)
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
        .commit_install(prepared)
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
