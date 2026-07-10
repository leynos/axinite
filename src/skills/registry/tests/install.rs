//! Deterministic regression tests for skill bundle installation correctness
//! and byte-for-byte file preservation across supported install transports.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use rstest::rstest;

use super::super::*;
use super::fixtures::{
    BundleInstallFixture, build_bundle_archive, bundle_install_fixture, skill_markdown,
};
use crate::skills::SkillPackageKind;

fn assert_deploy_docs_bundle_files_present(path: &Path) {
    assert!(path.join("SKILL.md").exists());
    assert!(path.join("references/usage.md").exists());
    assert!(path.join("assets/logo.txt").exists());
}

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

fn collect_installed_files(root: &Path) -> BTreeMap<PathBuf, Vec<u8>> {
    fn visit(base: &Path, current: &Path, files: &mut BTreeMap<PathBuf, Vec<u8>>) {
        for entry in ambient_fs::read_dir(current).expect("installed directory should be readable")
        {
            let entry = entry.expect("installed directory entry should be readable");
            let path = entry.path();
            if path.is_dir() {
                visit(base, &path, files);
            } else {
                let relative = path
                    .strip_prefix(base)
                    .expect("installed file should be under bundle root")
                    .to_path_buf();
                let contents = ambient_fs::read(&path).expect("installed file should be readable");
                files.insert(relative, contents);
            }
        }
    }

    let mut files = BTreeMap::new();
    visit(root, root, &mut files);
    files
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

    assert_eq!(
        prepared.loaded_skill.package_kind(),
        SkillPackageKind::Bundle
    );
    assert_eq!(
        prepared.loaded_skill.skill_root(),
        installed_dir.path().join("deploy-docs").as_path()
    );
    assert_eq!(
        prepared.loaded_skill.skill_entrypoint(),
        std::path::Path::new("SKILL.md")
    );

    registry.commit_install(prepared).expect(commit_msg);

    let installed_root = installed_dir.path().join("deploy-docs");
    assert_deploy_docs_bundle_files_present(&installed_root);
    assert!(registry.has("deploy-docs"));
    let skill = registry
        .find_by_name("deploy-docs")
        .expect("committed bundle skill should be loaded");
    assert_eq!(skill.package_kind(), SkillPackageKind::Bundle);
    assert_eq!(skill.skill_root(), installed_root.as_path());
}

/// RFC 0003 regression: installing a `.skill` bundle must preserve every
/// documented entry class instead of dropping references or assets.
#[rstest]
#[case::downloaded_bytes(|b| SkillInstallPayload::DownloadedBytes(b))]
#[case::archive_bytes(|b| SkillInstallPayload::ArchiveBytes(b))]
#[tokio::test]
async fn test_install_preserves_references_and_assets_regression_rfc0003(
    bundle_install_fixture: BundleInstallFixture,
    #[case] make_payload: impl FnOnce(Vec<u8>) -> SkillInstallPayload,
) {
    let BundleInstallFixture {
        installed_dir,
        mut registry,
        ..
    } = bundle_install_fixture;

    let entries = documented_bundle_entries();
    let archive = build_bundle_archive(&entries);
    let prepared = SkillRegistry::prepare_install_to_disk(
        registry.install_target_dir(),
        make_payload(archive),
    )
    .await
    .expect("documented bundle should prepare");
    assert_eq!(
        prepared.loaded_skill.package_kind(),
        SkillPackageKind::Bundle
    );

    registry
        .commit_install(prepared)
        .expect("documented bundle should commit");

    let installed_root = installed_dir.path().join("deploy-docs");
    let expected = entries
        .into_iter()
        .map(|(path, contents)| {
            (
                PathBuf::from(
                    path.strip_prefix("deploy-docs/")
                        .expect("manifest path should be bundle rooted"),
                ),
                contents.to_vec(),
            )
        })
        .collect::<BTreeMap<_, _>>();
    assert_eq!(collect_installed_files(&installed_root), expected);

    let skill = registry
        .find_by_name("deploy-docs")
        .expect("committed bundle skill should be loaded");
    assert_eq!(skill.package_kind(), SkillPackageKind::Bundle);
    assert_eq!(skill.skill_root(), installed_root.as_path());
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

    assert_eq!(
        prepared.loaded_skill.package_kind(),
        SkillPackageKind::SingleFile
    );
    assert_eq!(
        prepared.loaded_skill.skill_root(),
        installed_dir.path().join("deploy-docs").as_path()
    );
    assert_eq!(
        prepared.loaded_skill.skill_entrypoint(),
        std::path::Path::new("SKILL.md")
    );

    registry
        .commit_install(prepared)
        .expect("downloaded markdown should commit successfully");

    assert!(installed_dir.path().join("deploy-docs/SKILL.md").exists());
    assert!(registry.has("deploy-docs"));
    let skill = registry
        .find_by_name("deploy-docs")
        .expect("committed markdown skill should be loaded");
    assert_eq!(skill.package_kind(), SkillPackageKind::SingleFile);
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
    assert_deploy_docs_bundle_files_present(&installed_root);

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

    assert_deploy_docs_bundle_files_present(&installed_root);
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

    let install_root_entries = ambient_fs::read_dir(installed_dir.path())
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
