//! Install-payload tests: archive and downloaded-bytes transports must
//! preserve documented bundle entries and honour markdown acceptance rules.

use std::collections::BTreeMap;
use std::path::PathBuf;

use rstest::rstest;

use super::super::super::*;
use super::super::fixtures::{
    BundleInstallFixture, build_bundle_archive, bundle_install_fixture, skill_markdown,
};
use super::{
    assert_deploy_docs_bundle_files_present, collect_installed_files, documented_bundle_entries,
};
use crate::skills::SkillPackageKind;

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
