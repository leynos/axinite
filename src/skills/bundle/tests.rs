use std::io::Write;

use rstest::rstest;

use super::{
    MAX_BUNDLE_FILE_BYTES, MAX_BUNDLE_FILE_COUNT, MAX_BUNDLE_TOTAL_BYTES, SkillBundleError,
    looks_like_skill_archive, validate_skill_archive,
};
use crate::skills::MAX_PROMPT_FILE_SIZE;

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
    ]);

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
    ]),
    "missing <root>/SKILL.md"
)]
#[case::multiple_roots(
    build_bundle_archive(&[
        file_entry("deploy-docs/SKILL.md", skill_markdown("deploy-docs").as_bytes()),
        file_entry("other-skill/references/usage.md", b"# Usage\n"),
    ]),
    "expected one top-level path prefix"
)]
#[case::unexpected_top_level_file(
    build_bundle_archive(&[
        file_entry("deploy-docs/SKILL.md", skill_markdown("deploy-docs").as_bytes()),
        file_entry("deploy-docs/notes.md", b"# Notes\n"),
    ]),
    "unexpected path"
)]
#[case::nested_skill_md(
    build_bundle_archive(&[
        file_entry("deploy-docs/SKILL.md", skill_markdown("deploy-docs").as_bytes()),
        file_entry("deploy-docs/references/nested/SKILL.md", b"bad"),
    ]),
    "nested SKILL.md"
)]
#[case::scripts_dir(
    build_bundle_archive(&[
        file_entry("deploy-docs/SKILL.md", skill_markdown("deploy-docs").as_bytes()),
        file_entry("deploy-docs/scripts/install.sh", b"echo nope"),
    ]),
    "directory 'deploy-docs/scripts/install.sh' is not allowed"
)]
#[case::executable_extension(
    build_bundle_archive(&[
        file_entry("deploy-docs/SKILL.md", skill_markdown("deploy-docs").as_bytes()),
        file_entry("deploy-docs/assets/install.sh", b"echo nope"),
    ]),
    "executable payloads are not allowed"
)]
#[case::traversal(
    build_bundle_archive(&[
        file_entry("deploy-docs/SKILL.md", skill_markdown("deploy-docs").as_bytes()),
        file_entry("deploy-docs/../evil.md", b"bad"),
    ]),
    "expected one top-level path prefix"
)]
#[case::duplicate_casefold(
    build_bundle_archive(&[
        file_entry("deploy-docs/SKILL.md", skill_markdown("deploy-docs").as_bytes()),
        file_entry("deploy-docs/references/Guide.md", b"# A\n"),
        file_entry("deploy-docs/references/guide.md", b"# B\n"),
    ]),
    "duplicate path after normalization"
)]
#[case::oversize_skill_md(
    build_bundle_archive(&[
        file_entry(
            "deploy-docs/SKILL.md",
            format!("{}\n{}", skill_frontmatter("deploy-docs"), "x".repeat((MAX_PROMPT_FILE_SIZE + 1) as usize)).as_bytes(),
        ),
    ]),
    "SKILL.md' is"
)]
#[case::oversize_asset(
    build_bundle_archive(&[
        file_entry("deploy-docs/SKILL.md", skill_markdown("deploy-docs").as_bytes()),
        file_entry(
            "deploy-docs/assets/big.bin",
            &vec![0u8; (MAX_BUNDLE_FILE_BYTES + 1) as usize],
        ),
    ]),
    "big.bin' is"
)]
#[case::too_many_files(build_many_file_archive(MAX_BUNDLE_FILE_COUNT + 1), "contains 129 files")]
#[case::archive_too_large(build_total_size_archive(MAX_BUNDLE_TOTAL_BYTES + 1), "archive totals")]
#[case::invalid_reference_utf8(
    build_bundle_archive(&[
        file_entry("deploy-docs/SKILL.md", skill_markdown("deploy-docs").as_bytes()),
        file_entry("deploy-docs/references/usage.md", &[0xff, 0xfe]),
    ]),
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
    )]);

    let bundle = validate_skill_archive(&archive).expect("dotted bundle root should validate");
    assert_eq!(bundle.skill_name(), "deploy.docs.v2");
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
    ]);

    let error = validate_skill_archive(&archive).expect_err("executable mode should fail");
    assert!(matches!(
        error,
        SkillBundleError::ExecutablePayload { ref path } if path == "deploy-docs/assets/helper.txt"
    ));
}

fn skill_frontmatter(name: &str) -> String {
    format!("---\nname: {name}\n---")
}

fn skill_markdown(name: &str) -> String {
    format!("{}\n\n# {name}\n", skill_frontmatter(name))
}

#[derive(Clone)]
struct EntrySpec {
    name: String,
    data: Vec<u8>,
    unix_mode: Option<u32>,
}

fn file_entry(name: &str, data: &[u8]) -> EntrySpec {
    EntrySpec {
        name: name.to_string(),
        data: data.to_vec(),
        unix_mode: None,
    }
}

fn file_entry_with_mode(name: &str, data: &[u8], unix_mode: u32) -> EntrySpec {
    EntrySpec {
        name: name.to_string(),
        data: data.to_vec(),
        unix_mode: Some(unix_mode),
    }
}

fn build_bundle_archive(entries: &[EntrySpec]) -> Vec<u8> {
    let cursor = std::io::Cursor::new(Vec::new());
    let mut writer = zip::ZipWriter::new(cursor);
    let options = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated);

    for entry in entries {
        let mut entry_options = options;
        if let Some(unix_mode) = entry.unix_mode {
            entry_options = entry_options.unix_permissions(unix_mode);
        }
        writer
            .start_file(&entry.name, entry_options)
            .expect("test archive should start file");
        writer
            .write_all(&entry.data)
            .expect("test archive should accept file data");
    }

    writer
        .finish()
        .expect("test archive should finish")
        .into_inner()
}

fn build_many_file_archive(file_count: usize) -> Vec<u8> {
    let mut entries = vec![file_entry(
        "deploy-docs/SKILL.md",
        skill_markdown("deploy-docs").as_bytes(),
    )];

    for index in 0..file_count.saturating_sub(1) {
        entries.push(file_entry(
            &format!("deploy-docs/references/file-{index}.md"),
            b"# Ref\n",
        ));
    }

    build_bundle_archive(&entries)
}

fn build_total_size_archive(target_size: u64) -> Vec<u8> {
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
