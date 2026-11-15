//! Unit and property tests for skill bundle file read operations.

use std::path::PathBuf;

use insta::assert_json_snapshot;
use rstest::{fixture, rstest};
use tempfile::TempDir;

use super::*;
use crate::skills::test_support::TestSkillBuilder;
use crate::skills::{LoadedSkillLocation, SkillPackageKind};

// Every case in `io` is Linux-gated, so the module itself is compiled only
// there; importing it elsewhere would leave its imports unused.
#[cfg(target_os = "linux")]
mod io;
mod reads;
mod validation;

/// A loaded skill bundle backed by a temporary directory.
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

/// Require an error response carrying `expected`.
fn assert_error_code(response: SkillReadFileResponse, expected: SkillReadFileErrorCode) {
    let SkillReadFileResponse::Error(response) = response else {
        panic!("expected error response");
    };
    assert_eq!(response.error.code, expected);
}

// ── JSON shape snapshot tests ────────────────────────────────────────────────

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
    SkillReadFileResponse::error("deploy-docs", "../secret", error)
}

fn make_error_response(
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

fn snapshot_error_non_inline_asset() -> SkillReadFileResponse {
    make_error_response(
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
    make_error_response(
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
    make_error_response(
        "references/binary.md",
        SkillReadFileErrorCode::InvalidUtf8,
        "File is not valid UTF-8 text",
        None,
    )
}

fn snapshot_error_io_error() -> SkillReadFileResponse {
    make_error_response(
        "references/usage.md",
        SkillReadFileErrorCode::IoError,
        "File is not available for reading",
        None,
    )
}
