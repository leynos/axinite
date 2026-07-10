//! Bundle validation tests for passive `.skill` archives.
//!
//! These tests cover archive sniffing via `looks_like_skill_archive`,
//! structural and content validation through `validate_skill_archive`, and the
//! enforced bundle limits governed by `MAX_BUNDLE_FILE_BYTES`,
//! `MAX_BUNDLE_FILE_COUNT`, `MAX_BUNDLE_TOTAL_BYTES`, and
//! `MAX_PROMPT_FILE_SIZE`.

use rstest::rstest;

use super::{
    MAX_BUNDLE_FILE_BYTES, MAX_BUNDLE_FILE_COUNT, MAX_BUNDLE_TOTAL_BYTES, SkillBundleError,
    looks_like_skill_archive, validate_skill_archive,
};
use crate::skills::MAX_PROMPT_FILE_SIZE;
use crate::skills::test_support::{BundleArchiveEntry, build_bundle_archive_from_entries};

fn build_bundle_archive(entries: &[BundleArchiveEntry]) -> Result<Vec<u8>, zip::result::ZipError> {
    build_bundle_archive_from_entries(entries)
}

#[test]
fn looks_like_zip_signatures_detect_skill_archives() {
    assert!(looks_like_skill_archive(b"PK\x03\x04rest"));
    assert!(looks_like_skill_archive(b"PK\x05\x06rest"));
    assert!(!looks_like_skill_archive(b"---\nname: skill\n---\n"));
}

#[test]
fn validates_bundle_with_references_and_assets() {
    let archive = build_bundle_archive(&[
        file_entry(
            "deploy-docs/SKILL.md",
            skill_markdown("deploy-docs").as_bytes(),
        ),
        file_entry(
            "deploy-docs/references/usage.md",
            b"# Usage\nUse the documented process.\n",
        ),
        file_entry("deploy-docs/assets/logo.png", &[0x89, b'P', b'N', b'G']),
    ])
    .expect("test bundle archive should build");

    let bundle = validate_skill_archive(&archive).expect("valid bundle should pass");

    assert_eq!(bundle.skill_name(), "deploy-docs");
    let paths: Vec<String> = bundle
        .entries()
        .iter()
        .map(|entry| entry.relative_path().display().to_string())
        .collect();
    assert_eq!(
        paths,
        vec![
            "SKILL.md".to_string(),
            "references/usage.md".to_string(),
            "assets/logo.png".to_string()
        ]
    );
}

#[rstest]
#[case::missing_root_skill(
    build_bundle_archive(&[
        file_entry("deploy-docs/references/usage.md", b"# Usage\n"),
    ]).expect("test bundle archive should build"),
    "missing <root>/SKILL.md"
)]
#[case::multiple_roots(
    build_bundle_archive(&[
        file_entry("deploy-docs/SKILL.md", skill_markdown("deploy-docs").as_bytes()),
        file_entry("other-skill/references/usage.md", b"# Usage\n"),
    ]).expect("test bundle archive should build"),
    "expected one top-level path prefix"
)]
#[case::unexpected_top_level_file(
    build_bundle_archive(&[
        file_entry("deploy-docs/SKILL.md", skill_markdown("deploy-docs").as_bytes()),
        file_entry("deploy-docs/notes.md", b"# Notes\n"),
    ]).expect("test bundle archive should build"),
    "unexpected path"
)]
#[case::nested_skill_md(
    build_bundle_archive(&[
        file_entry("deploy-docs/SKILL.md", skill_markdown("deploy-docs").as_bytes()),
        file_entry("deploy-docs/references/nested/SKILL.md", b"bad"),
    ]).expect("test bundle archive should build"),
    "nested SKILL.md"
)]
#[case::scripts_dir(
    build_bundle_archive(&[
        file_entry("deploy-docs/SKILL.md", skill_markdown("deploy-docs").as_bytes()),
        file_entry("deploy-docs/scripts/install.sh", b"echo nope"),
    ]).expect("test bundle archive should build"),
    "entry 'deploy-docs/scripts/install.sh' is not allowed"
)]
#[case::executable_extension(
    build_bundle_archive(&[
        file_entry("deploy-docs/SKILL.md", skill_markdown("deploy-docs").as_bytes()),
        file_entry("deploy-docs/assets/install.sh", b"echo nope"),
    ]).expect("test bundle archive should build"),
    "executable payloads are not allowed"
)]
#[case::traversal(
    build_bundle_archive(&[
        file_entry("deploy-docs/SKILL.md", skill_markdown("deploy-docs").as_bytes()),
        file_entry("deploy-docs/../evil.md", b"bad"),
    ]).expect("test bundle archive should build"),
    "expected one top-level path prefix"
)]
#[case::duplicate_casefold(
    build_bundle_archive(&[
        file_entry("deploy-docs/SKILL.md", skill_markdown("deploy-docs").as_bytes()),
        file_entry("deploy-docs/references/Guide.md", b"# A\n"),
        file_entry("deploy-docs/references/guide.md", b"# B\n"),
    ]).expect("test bundle archive should build"),
    "duplicate path after normalization"
)]
#[case::oversize_skill_md(
    build_bundle_archive(&[
        file_entry(
            "deploy-docs/SKILL.md",
            format!("{}\n{}", skill_frontmatter("deploy-docs"), "x".repeat((MAX_PROMPT_FILE_SIZE + 1) as usize)).as_bytes(),
        ),
    ]).expect("test bundle archive should build"),
    "SKILL.md' is"
)]
#[case::oversize_asset(
    build_bundle_archive(&[
        file_entry("deploy-docs/SKILL.md", skill_markdown("deploy-docs").as_bytes()),
        file_entry(
            "deploy-docs/assets/big.bin",
            &vec![0u8; (MAX_BUNDLE_FILE_BYTES + 1) as usize],
        ),
    ]).expect("test bundle archive should build"),
    "big.bin' is"
)]
#[case::too_many_files(
    build_many_file_archive(MAX_BUNDLE_FILE_COUNT + 1)
        .expect("test bundle archive should build"),
    "contains 129 files"
)]
#[case::archive_too_large(
    build_total_size_archive(MAX_BUNDLE_TOTAL_BYTES + 1)
        .expect("test bundle archive should build"),
    "archive totals"
)]
#[case::invalid_reference_utf8(
    build_bundle_archive(&[
        file_entry("deploy-docs/SKILL.md", skill_markdown("deploy-docs").as_bytes()),
        file_entry("deploy-docs/references/usage.md", &[0xff, 0xfe]),
    ]).expect("test bundle archive should build"),
    "not valid UTF-8"
)]
fn rejects_invalid_bundles(#[case] archive: Vec<u8>, #[case] expected_message: &str) {
    let error = validate_skill_archive(&archive).expect_err("bundle should be rejected");
    assert!(
        error.to_string().contains(expected_message),
        "expected error containing '{expected_message}', got: {error}",
    );
}

#[test]
fn dotted_root_names_remain_valid() {
    let archive = build_bundle_archive(&[file_entry(
        "deploy.docs.v2/SKILL.md",
        skill_markdown("deploy.docs.v2").as_bytes(),
    )])
    .expect("test bundle archive should build");

    let bundle = validate_skill_archive(&archive).expect("dotted bundle root should validate");
    assert_eq!(bundle.skill_name(), "deploy.docs.v2");
}

#[test]
fn malformed_paths_are_rejected() {
    let malformed_paths = ["\\windows\\style\\path.txt", "/absolute/path.txt"];

    for path in malformed_paths {
        let archive = build_bundle_archive(&[file_entry(path, b"malformed path")])
            .expect("test bundle archive should build");

        let error =
            validate_skill_archive(&archive).expect_err("malformed raw path should be rejected");
        assert!(
            matches!(error, SkillBundleError::InvalidTopLevelPrefix),
            "expected InvalidTopLevelPrefix for path {path:?}, got {error:?}"
        );
    }
}

#[test]
fn directory_entries_under_references_and_assets_are_accepted() {
    let archive = build_bundle_archive(&[
        file_entry(
            "deploy-docs/SKILL.md",
            skill_markdown("deploy-docs").as_bytes(),
        ),
        file_entry("deploy-docs/references/", &[]),
        file_entry("deploy-docs/assets/", &[]),
    ])
    .expect("test bundle archive should build");

    validate_skill_archive(&archive)
        .expect("directory-only references/ and assets/ entries should be accepted");
}

#[test]
fn bare_references_and_assets_files_are_rejected() {
    for path in ["deploy-docs/references", "deploy-docs/assets"] {
        let archive = build_bundle_archive(&[
            file_entry(
                "deploy-docs/SKILL.md",
                skill_markdown("deploy-docs").as_bytes(),
            ),
            file_entry(path, b"not a directory"),
        ])
        .expect("test bundle archive should build");

        let error =
            validate_skill_archive(&archive).expect_err("bare file entry should be rejected");
        assert!(
            matches!(error, SkillBundleError::UnexpectedPath { path: ref rejected_path } if rejected_path == path),
            "expected UnexpectedPath for bare file entry, got {error:?}"
        );
    }
}

#[test]
fn unix_executable_bits_are_rejected() {
    let archive = build_bundle_archive(&[
        file_entry(
            "deploy-docs/SKILL.md",
            skill_markdown("deploy-docs").as_bytes(),
        ),
        file_entry_with_mode(
            "deploy-docs/assets/helper.txt",
            b"text with executable mode",
            0o100755,
        ),
    ])
    .expect("test bundle archive should build");

    let error = validate_skill_archive(&archive).expect_err("executable mode should fail");
    assert!(matches!(
        error,
        SkillBundleError::ExecutablePayload { ref path } if path == "deploy-docs/assets/helper.txt"
    ));
}

#[test]
fn unsupported_unix_file_types_are_rejected() {
    let archive = build_bundle_archive_with_symlink("deploy-docs/SKILL.md", "../outside-target")
        .expect("symlink test archive should build");

    let error =
        validate_skill_archive(&archive).expect_err("unsupported Unix file type should fail");
    assert!(matches!(
        error,
        SkillBundleError::UnsupportedFileType { ref path } if path == "deploy-docs/SKILL.md"
    ));
}

fn skill_frontmatter(name: impl AsRef<str>) -> String {
    let name = name.as_ref();
    format!("---\nname: {name}\n---")
}

fn skill_markdown(name: impl AsRef<str>) -> String {
    let name = name.as_ref();
    format!("{}\n\n# {name}\n", skill_frontmatter(name))
}

fn file_entry(name: impl AsRef<str>, data: &[u8]) -> EntrySpec {
    BundleArchiveEntry::file(name, data)
}

fn file_entry_with_mode(name: impl AsRef<str>, data: &[u8], unix_mode: u32) -> EntrySpec {
    BundleArchiveEntry::file_with_mode(name, data, unix_mode)
}

type EntrySpec = BundleArchiveEntry;

fn build_bundle_archive_with_symlink(
    name: impl AsRef<str>,
    target: impl AsRef<str>,
) -> Result<Vec<u8>, zip::result::ZipError> {
    let cursor = std::io::Cursor::new(Vec::new());
    let mut writer = zip::ZipWriter::new(cursor);

    writer.add_symlink(
        name.as_ref(),
        target.as_ref(),
        zip::write::SimpleFileOptions::default(),
    )?;

    Ok(writer.finish()?.into_inner())
}

fn build_many_file_archive(file_count: usize) -> Result<Vec<u8>, zip::result::ZipError> {
    let mut entries = vec![file_entry(
        "deploy-docs/SKILL.md",
        skill_markdown("deploy-docs").as_bytes(),
    )];

    for index in 0..file_count.saturating_sub(1) {
        entries.push(file_entry(
            format!("deploy-docs/references/file-{index}.md"),
            b"# Ref\n",
        ));
    }

    build_bundle_archive(&entries)
}

fn build_total_size_archive(target_size: u64) -> Result<Vec<u8>, zip::result::ZipError> {
    let payload_len = target_size
        .saturating_sub(skill_markdown("deploy-docs").len() as u64)
        .saturating_add(1) as usize;
    build_bundle_archive(&[
        file_entry(
            "deploy-docs/SKILL.md",
            skill_markdown("deploy-docs").as_bytes(),
        ),
        file_entry(
            "deploy-docs/assets/blob.bin",
            &vec![0u8; payload_len.min(MAX_BUNDLE_FILE_BYTES as usize)],
        ),
        file_entry(
            "deploy-docs/assets/blob-2.bin",
            &vec![0u8; payload_len.min(MAX_BUNDLE_FILE_BYTES as usize)],
        ),
    ])
}
