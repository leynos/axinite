//! Behavioural tests for reading skill bundle files, covering allowed
//! reads, path rejection, size limits, and symlink handling.

use std::path::PathBuf;

use rstest::{fixture, rstest};
use tempfile::TempDir;

use super::super::*;
use crate::skills::test_support::{TestSkillBuilder, installed_bundle_fixture};
use crate::skills::{LoadedSkillLocation, SkillPackageKind};

struct SkillReadFixture {
    _dir: TempDir,
    skill: LoadedSkill,
}

#[fixture]
fn skill_read_fixture() -> anyhow::Result<SkillReadFixture> {
    use anyhow::Context as _;

    let dir = tempfile::tempdir().context("tempdir should be created")?;
    ambient_fs::create_dir_all(dir.path().join("references"))
        .context("references dir should be created")?;
    ambient_fs::create_dir_all(dir.path().join("assets"))
        .context("assets dir should be created")?;
    ambient_fs::write(dir.path().join("SKILL.md"), "# Deploy docs\n")
        .context("skill prompt should be written")?;
    ambient_fs::write(dir.path().join("references/usage.md"), "# Usage\n")
        .context("reference should be written")?;
    ambient_fs::write(dir.path().join("assets/note.txt"), "asset notes\n")
        .context("text asset should be written")?;
    ambient_fs::write(
        dir.path().join("assets/logo.png"),
        [0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A],
    )
    .context("binary asset should be written")?;

    let location = LoadedSkillLocation::new(
        "deploy-docs",
        dir.path(),
        PathBuf::from("SKILL.md"),
        SkillPackageKind::Bundle,
    )
    .context("test location should be valid")?;
    let skill = TestSkillBuilder::new("deploy-docs")
        .location(location)
        .build()?;

    Ok(SkillReadFixture { _dir: dir, skill })
}

fn installed_read_entries() -> Vec<(&'static str, &'static [u8])> {
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

#[rstest]
#[case::entrypoint(
    "SKILL.md",
    "text/markdown",
    "---\nname: deploy-docs\n---\n\n# deploy-docs\n"
)]
#[case::reference("references/usage.md", "text/markdown", "# Usage\n")]
#[case::nested_reference("references/nested/api.md", "text/markdown", "# API\n")]
#[case::text_asset("assets/note.txt", "text/plain", "asset notes\n")]
#[tokio::test]
#[cfg(target_os = "linux")]
async fn test_read_skill_file_after_install_returns_each_text_entry(
    #[case] path: &str,
    #[case] mime_type: &str,
    #[case] content: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    let fixture = installed_bundle_fixture(&installed_read_entries()).await?;

    let response = read_skill_file(&fixture.loaded_skill, path).await;

    assert_eq!(
        response,
        SkillReadFileResponse::Success(SkillReadFileSuccess {
            skill: "deploy-docs".to_string(),
            path: path.to_string(),
            mime_type: mime_type.to_string(),
            content: content.to_string(),
        })
    );
    Ok(())
}

#[rstest]
#[tokio::test]
#[cfg(target_os = "linux")]
async fn test_read_skill_file_after_install_returns_non_inline_metadata_for_binary()
-> Result<(), Box<dyn std::error::Error>> {
    let fixture = installed_bundle_fixture(&installed_read_entries()).await?;

    let response = read_skill_file(&fixture.loaded_skill, "assets/logo.png").await;

    let SkillReadFileResponse::Error(response) = response else {
        panic!("binary asset should return a typed error payload");
    };
    assert_eq!(response.error.code, SkillReadFileErrorCode::NonInlineAsset);
    assert_eq!(
        response.error.metadata.expect("metadata should be present"),
        SkillReadFileMetadata {
            size: 8,
            mime_type: "image/png".to_string(),
            fetch_hint: NON_INLINE_FETCH_HINT.to_string(),
        }
    );
    Ok(())
}

#[rstest]
#[tokio::test]
#[cfg(not(target_os = "linux"))]
async fn test_read_skill_file_after_install_returns_io_error_on_non_linux()
-> Result<(), Box<dyn std::error::Error>> {
    let fixture = installed_bundle_fixture(&installed_read_entries()).await?;

    let response = read_skill_file(&fixture.loaded_skill, "references/usage.md").await;

    assert_error_code(response, SkillReadFileErrorCode::IoError);
    Ok(())
}

#[rstest]
#[tokio::test]
#[cfg(target_os = "linux")]
async fn reads_bundle_reference_text(skill_read_fixture: anyhow::Result<SkillReadFixture>) {
    let skill_read_fixture = skill_read_fixture.expect("skill read fixture should build");
    let response = read_skill_file(&skill_read_fixture.skill, "references/usage.md").await;

    assert_eq!(
        response,
        SkillReadFileResponse::Success(SkillReadFileSuccess {
            skill: "deploy-docs".to_string(),
            path: "references/usage.md".to_string(),
            mime_type: "text/markdown".to_string(),
            content: "# Usage\n".to_string(),
        })
    );
}

#[rstest]
#[tokio::test]
#[cfg(target_os = "linux")]
async fn reads_bundle_reference_text_at_max_size(
    skill_read_fixture: anyhow::Result<SkillReadFixture>,
) {
    let skill_read_fixture = skill_read_fixture.expect("skill read fixture should build");
    ambient_fs::write(
        skill_read_fixture
            .skill
            .skill_root()
            .join("references/max-size.md"),
        "x".repeat(MAX_SKILL_READ_FILE_BYTES as usize),
    )
    .expect("max-size reference should be written");

    let response = read_skill_file(&skill_read_fixture.skill, "references/max-size.md").await;

    let SkillReadFileResponse::Success(success) = response else {
        panic!("max-size reference should be returned inline");
    };
    assert_eq!(success.skill, "deploy-docs");
    assert_eq!(success.path, "references/max-size.md");
    assert_eq!(success.mime_type, "text/markdown");
    assert_eq!(success.content.len(), MAX_SKILL_READ_FILE_BYTES as usize);
}

#[rstest]
#[tokio::test]
#[cfg(target_os = "linux")]
async fn reads_skill_entrypoint(skill_read_fixture: anyhow::Result<SkillReadFixture>) {
    let skill_read_fixture = skill_read_fixture.expect("skill read fixture should build");
    let response = read_skill_file(&skill_read_fixture.skill, "SKILL.md").await;

    assert_eq!(
        response,
        SkillReadFileResponse::Success(SkillReadFileSuccess {
            skill: "deploy-docs".to_string(),
            path: "SKILL.md".to_string(),
            mime_type: "text/markdown".to_string(),
            content: "# Deploy docs\n".to_string(),
        })
    );
}

#[rstest]
#[case::absolute("/etc/passwd")]
#[case::traversal("../secret")]
#[case::nested_entrypoint("references/SKILL.md")]
#[case::unsupported_root("scripts/install.sh")]
#[case::windows_separator("references\\usage.md")]
#[case::bare_references_directory("references/")]
#[case::bare_assets_directory("assets/")]
#[tokio::test]
async fn rejects_disallowed_paths(
    skill_read_fixture: anyhow::Result<SkillReadFixture>,
    #[case] path: &str,
) {
    let skill_read_fixture = skill_read_fixture.expect("skill read fixture should build");
    let response = read_skill_file(&skill_read_fixture.skill, path).await;

    assert_error_code(response, SkillReadFileErrorCode::PathNotReadable);
}

#[rstest]
#[tokio::test]
#[cfg(target_os = "linux")]
async fn binary_asset_returns_non_inline_metadata(
    skill_read_fixture: anyhow::Result<SkillReadFixture>,
) {
    let skill_read_fixture = skill_read_fixture.expect("skill read fixture should build");
    let response = read_skill_file(&skill_read_fixture.skill, "assets/logo.png").await;

    let SkillReadFileResponse::Error(response) = response else {
        panic!("binary asset should return a typed error payload");
    };
    assert_eq!(response.error.code, SkillReadFileErrorCode::NonInlineAsset);
    assert_eq!(
        response
            .error
            .metadata
            .expect("metadata should be present")
            .mime_type,
        "image/png"
    );
}

#[rstest]
#[tokio::test]
#[cfg(target_os = "linux")]
async fn oversized_text_returns_file_too_large(
    skill_read_fixture: anyhow::Result<SkillReadFixture>,
) {
    let skill_read_fixture = skill_read_fixture.expect("skill read fixture should build");
    ambient_fs::write(
        skill_read_fixture
            .skill
            .skill_root()
            .join("references/large.md"),
        "x".repeat(MAX_SKILL_READ_FILE_BYTES as usize + 1),
    )
    .expect("large reference should be written");

    let response = read_skill_file(&skill_read_fixture.skill, "references/large.md").await;

    assert_error_code(response, SkillReadFileErrorCode::FileTooLarge);
}

#[rstest]
#[tokio::test]
#[cfg(target_os = "linux")]
async fn missing_file_returns_path_not_readable(
    skill_read_fixture: anyhow::Result<SkillReadFixture>,
) {
    let skill_read_fixture = skill_read_fixture.expect("skill read fixture should build");
    let response = read_skill_file(&skill_read_fixture.skill, "references/missing.md").await;

    assert_error_code(response, SkillReadFileErrorCode::PathNotReadable);
}

#[cfg(target_os = "linux")]
#[rstest]
#[tokio::test]
async fn symlink_paths_are_rejected(skill_read_fixture: anyhow::Result<SkillReadFixture>) {
    let skill_read_fixture = skill_read_fixture.expect("skill read fixture should build");
    std::os::unix::fs::symlink(
        skill_read_fixture
            .skill
            .skill_root()
            .join("references/usage.md"),
        skill_read_fixture
            .skill
            .skill_root()
            .join("references/link.md"),
    )
    .expect("symlink should be created");

    let response = read_skill_file(&skill_read_fixture.skill, "references/link.md").await;

    assert_error_code(response, SkillReadFileErrorCode::PathNotReadable);
}

#[cfg(target_os = "linux")]
#[rstest]
#[tokio::test]
async fn intermediate_symlink_paths_are_rejected(
    skill_read_fixture: anyhow::Result<SkillReadFixture>,
) {
    let skill_read_fixture = skill_read_fixture.expect("skill read fixture should build");
    let external = tempfile::tempdir().expect("external tempdir should be created");
    ambient_fs::write(external.path().join("usage.md"), "# Escaped\n")
        .expect("external reference should be written");
    std::os::unix::fs::symlink(
        external.path(),
        skill_read_fixture
            .skill
            .skill_root()
            .join("references/external"),
    )
    .expect("intermediate symlink should be created");

    let response = read_skill_file(&skill_read_fixture.skill, "references/external/usage.md").await;

    assert_error_code(response, SkillReadFileErrorCode::PathNotReadable);
}

#[cfg(not(target_os = "linux"))]
#[rstest]
#[case::reference("references/usage.md")]
#[case::entrypoint("SKILL.md")]
#[tokio::test]
async fn allowed_reads_fail_closed_on_non_linux(
    skill_read_fixture: anyhow::Result<SkillReadFixture>,
    #[case] path: &str,
) {
    let skill_read_fixture = skill_read_fixture.expect("skill read fixture should build");
    let response = read_skill_file(&skill_read_fixture.skill, path).await;

    assert_error_code(response, SkillReadFileErrorCode::IoError);
}

fn assert_error_code(response: SkillReadFileResponse, expected: SkillReadFileErrorCode) {
    let SkillReadFileResponse::Error(response) = response else {
        panic!("expected error response");
    };
    assert_eq!(response.error.code, expected);
}
