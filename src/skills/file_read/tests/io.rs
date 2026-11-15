//! Behavioural tests for the `openat2` resolution that scopes a read to the
//! skill root: relative symlinks, escaping symlinks and interior NULs.

use std::path::Path;

use rstest::rstest;

use super::super::*;
use super::{SkillReadFixture, assert_error_code, skill_read_fixture};

#[cfg(target_os = "linux")]
#[rstest]
#[tokio::test]
async fn a_relative_symlink_inside_the_root_is_rejected(
    skill_read_fixture: anyhow::Result<SkillReadFixture>,
) -> anyhow::Result<()> {
    let skill_read_fixture = skill_read_fixture?;
    // The target stays beneath the skill root, so `RESOLVE_BENEATH` alone
    // admits it; only `RESOLVE_NO_SYMLINKS` rejects this path.
    std::os::unix::fs::symlink(
        "usage.md",
        skill_read_fixture
            .skill
            .skill_root()
            .join("references/relative.md"),
    )
    .expect("relative symlink should be created");

    let response = read_skill_file(&skill_read_fixture.skill, "references/relative.md").await;

    assert_error_code(response, SkillReadFileErrorCode::PathNotReadable);
    Ok(())
}

#[cfg(target_os = "linux")]
#[rstest]
#[tokio::test]
async fn an_escaping_relative_symlink_is_rejected(
    skill_read_fixture: anyhow::Result<SkillReadFixture>,
) -> anyhow::Result<()> {
    let skill_read_fixture = skill_read_fixture?;
    let external = tempfile::tempdir().expect("external tempdir should be created");
    ambient_fs::write(external.path().join("outside.md"), "# Escaped\n")
        .expect("external file should be written");
    // Resolve out of `references/` and into a sibling of the skill root, so
    // the link stays relative yet leaves the root.
    let sibling = external
        .path()
        .file_name()
        .expect("external tempdir name should be present");
    let target = Path::new("../../").join(sibling).join("outside.md");
    std::os::unix::fs::symlink(
        target,
        skill_read_fixture
            .skill
            .skill_root()
            .join("references/escape.md"),
    )
    .expect("escaping symlink should be created");

    let response = read_skill_file(&skill_read_fixture.skill, "references/escape.md").await;

    assert_error_code(response, SkillReadFileErrorCode::PathNotReadable);
    Ok(())
}

#[cfg(target_os = "linux")]
#[rstest]
#[tokio::test]
async fn an_interior_nul_in_the_path_is_rejected(
    skill_read_fixture: anyhow::Result<SkillReadFixture>,
) -> anyhow::Result<()> {
    let skill_read_fixture = skill_read_fixture?;
    // Validation admits this as a `references/` child, so the denial comes
    // from the `CString` conversion rather than from an `openat2` error.
    let response = read_skill_file(&skill_read_fixture.skill, "references/usage\0.md").await;

    assert_error_code(response, SkillReadFileErrorCode::PathNotReadable);
    Ok(())
}
