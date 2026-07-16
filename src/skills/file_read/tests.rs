//! Unit and property tests for skill bundle file read operations.

use insta::assert_json_snapshot;
use rstest::rstest;

use super::*;

mod reads;
mod validation;

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
