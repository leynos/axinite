//! Unit and property tests for skill bundle file read operations.

use std::path::PathBuf;

use insta::assert_json_snapshot;
use proptest::prelude::*;
use rstest::{fixture, rstest};
use tempfile::TempDir;

use super::*;
use crate::skills::test_support::TestSkillBuilder;
use crate::skills::{LoadedSkillLocation, SkillPackageKind};

struct SkillReadFixture {
    _dir: TempDir,
    skill: LoadedSkill,
}

#[fixture]
fn skill_read_fixture() -> SkillReadFixture {
    let dir = tempfile::tempdir().expect("tempdir should be created");
    std::fs::create_dir_all(dir.path().join("references"))
        .expect("references dir should be created");
    std::fs::create_dir_all(dir.path().join("assets")).expect("assets dir should be created");
    std::fs::write(dir.path().join("SKILL.md"), "# Deploy docs\n")
        .expect("skill prompt should be written");
    std::fs::write(dir.path().join("references/usage.md"), "# Usage\n")
        .expect("reference should be written");
    std::fs::write(dir.path().join("assets/note.txt"), "asset notes\n")
        .expect("text asset should be written");
    std::fs::write(
        dir.path().join("assets/logo.png"),
        [0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A],
    )
    .expect("binary asset should be written");

    let location = LoadedSkillLocation::new(
        "deploy-docs",
        dir.path(),
        PathBuf::from("SKILL.md"),
        SkillPackageKind::Bundle,
    )
    .expect("test location should be valid");
    let skill = TestSkillBuilder::new("deploy-docs")
        .location(location)
        .build();

    SkillReadFixture { _dir: dir, skill }
}

#[rstest]
#[tokio::test]
#[cfg(target_os = "linux")]
async fn reads_bundle_reference_text(skill_read_fixture: SkillReadFixture) {
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
async fn reads_bundle_reference_text_at_max_size(skill_read_fixture: SkillReadFixture) {
    std::fs::write(
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
async fn reads_skill_entrypoint(skill_read_fixture: SkillReadFixture) {
    let response = read_skill_file(&skill_read_fixture.skill, "SKILL.md").await;

    assert!(matches!(response, SkillReadFileResponse::Success(_)));
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
async fn rejects_disallowed_paths(skill_read_fixture: SkillReadFixture, #[case] path: &str) {
    let response = read_skill_file(&skill_read_fixture.skill, path).await;

    assert_error_code(response, SkillReadFileErrorCode::PathNotReadable);
}

#[rstest]
#[tokio::test]
#[cfg(target_os = "linux")]
async fn binary_asset_returns_non_inline_metadata(skill_read_fixture: SkillReadFixture) {
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
async fn oversized_text_returns_file_too_large(skill_read_fixture: SkillReadFixture) {
    std::fs::write(
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
async fn missing_file_returns_path_not_readable(skill_read_fixture: SkillReadFixture) {
    let response = read_skill_file(&skill_read_fixture.skill, "references/missing.md").await;

    assert_error_code(response, SkillReadFileErrorCode::PathNotReadable);
}

#[cfg(target_os = "linux")]
#[rstest]
#[tokio::test]
async fn symlink_paths_are_rejected(skill_read_fixture: SkillReadFixture) {
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
async fn intermediate_symlink_paths_are_rejected(skill_read_fixture: SkillReadFixture) {
    let external = tempfile::tempdir().expect("external tempdir should be created");
    std::fs::write(external.path().join("usage.md"), "# Escaped\n")
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
    skill_read_fixture: SkillReadFixture,
    #[case] path: &str,
) {
    let response = read_skill_file(&skill_read_fixture.skill, path).await;

    assert_error_code(response, SkillReadFileErrorCode::IoError);
}

fn assert_error_code(response: SkillReadFileResponse, expected: SkillReadFileErrorCode) {
    let SkillReadFileResponse::Error(response) = response else {
        panic!("expected error response");
    };
    assert_eq!(response.error.code, expected);
}

proptest! {
    #[test]
    fn allowed_reference_paths_validate(file_stem in "[a-z][a-z0-9_-]{0,32}") {
        let path = format!("references/{file_stem}.md");
        prop_assert!(validate_bundle_relative_path(&path).is_ok());
    }

    #[test]
    fn allowed_asset_paths_validate(file_stem in "[a-z][a-z0-9_-]{0,32}") {
        let path = format!("assets/{file_stem}.txt");
        prop_assert!(validate_bundle_relative_path(&path).is_ok());
    }

    #[test]
    fn disallowed_generated_paths_do_not_validate(raw in "\\PC*") {
        let looks_allowed = raw == "SKILL.md"
            || raw.starts_with("references/")
            || raw.starts_with("assets/");

        if !looks_allowed
            || raw.contains("..")
            || raw.contains('\\')
            || raw.starts_with('/')
            || raw.trim().is_empty()
        {
            prop_assert!(validate_bundle_relative_path(&raw).is_err());
        }
    }
}

#[test]
fn skill_entrypoint_path_validates() {
    assert!(validate_bundle_relative_path("SKILL.md").is_ok());
}

// ── JSON shape snapshot tests ────────────────────────────────────────────────

fn snapshot_error_response(
    path: &str,
    code: SkillReadFileErrorCode,
    message: &str,
    metadata: Option<SkillReadFileMetadata>,
) -> SkillReadFileResponse {
    SkillReadFileResponse::Error(SkillReadFileErrorResponse {
        skill: "deploy-docs".to_string(),
        path: path.to_string(),
        error: SkillReadFileError {
            code,
            message: message.to_string(),
            metadata,
        },
    })
}

#[rstest]
#[case::success("skill_read_file_success", snapshot_success_response())]
#[case::unknown_skill("skill_read_file_error_unknown_skill", snapshot_error_unknown_skill())]
#[case::path_not_readable(
    "skill_read_file_error_path_not_readable",
    snapshot_error_path_not_readable()
)]
#[case::non_inline_asset(
    "skill_read_file_error_non_inline_asset",
    snapshot_error_non_inline_asset()
)]
#[case::file_too_large(
    "skill_read_file_error_file_too_large",
    snapshot_error_file_too_large()
)]
#[case::invalid_utf8("skill_read_file_error_invalid_utf8", snapshot_error_invalid_utf8())]
#[case::io_error("skill_read_file_error_io_error", snapshot_error_io_error())]
fn snapshot_skill_read_file_response_shapes(
    #[case] snapshot_name: &str,
    #[case] response: SkillReadFileResponse,
) {
    assert_json_snapshot!(snapshot_name, &response);
}

fn snapshot_success_response() -> SkillReadFileResponse {
    SkillReadFileResponse::Success(SkillReadFileSuccess {
        skill: "deploy-docs".to_string(),
        path: "references/usage.md".to_string(),
        mime_type: "text/markdown".to_string(),
        content: "# Usage\n".to_string(),
    })
}

fn snapshot_error_unknown_skill() -> SkillReadFileResponse {
    SkillReadFileResponse::unknown_skill("deploy-docs", "references/usage.md")
}

fn snapshot_error_path_not_readable() -> SkillReadFileResponse {
    let error = validate_bundle_relative_path("../secret")
        .expect_err("traversal path should fail validation");
    let SkillReadFileError {
        code,
        message,
        metadata,
    } = error;
    snapshot_error_response("../secret", code, &message, metadata)
}

fn snapshot_error_non_inline_asset() -> SkillReadFileResponse {
    snapshot_error_response(
        "assets/logo.png",
        SkillReadFileErrorCode::NonInlineAsset,
        "Phase 1 does not return binary or oversized assets inline.",
        Some(SkillReadFileMetadata {
            size: 8,
            mime_type: "image/png".to_string(),
            fetch_hint: NON_INLINE_FETCH_HINT.to_string(),
        }),
    )
}

fn snapshot_error_file_too_large() -> SkillReadFileResponse {
    snapshot_error_response(
        "references/large.md",
        SkillReadFileErrorCode::FileTooLarge,
        "Phase 1 does not return binary or oversized assets inline.",
        Some(SkillReadFileMetadata {
            size: MAX_SKILL_READ_FILE_BYTES + 1,
            mime_type: "text/markdown".to_string(),
            fetch_hint: NON_INLINE_FETCH_HINT.to_string(),
        }),
    )
}

fn snapshot_error_invalid_utf8() -> SkillReadFileResponse {
    snapshot_error_response(
        "references/binary.md",
        SkillReadFileErrorCode::InvalidUtf8,
        "File is not valid UTF-8 text",
        None,
    )
}

fn snapshot_error_io_error() -> SkillReadFileResponse {
    snapshot_error_response(
        "references/usage.md",
        SkillReadFileErrorCode::IoError,
        "File is not available for reading",
        None,
    )
}
