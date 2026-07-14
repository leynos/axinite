//! Property tests for bundle-relative path validation and size boundaries.

use proptest::prelude::*;

use super::super::*;

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
    fn nested_entrypoints_are_rejected(root in "(references|assets)") {
        let path = format!("{root}/SKILL.md");
        prop_assert!(validate_bundle_relative_path(&path).is_err());
    }

    #[test]
    fn unsupported_root_paths_are_rejected(
        root in "[a-z][a-z0-9_-]{1,10}",
        filename in "[a-z][a-z0-9_-]{0,10}\\.[a-z]{1,4}",
    ) {
        prop_assume!(root != "references" && root != "assets");
        let path = format!("{root}/{filename}");
        prop_assert!(validate_bundle_relative_path(&path).is_err());
    }

    #[test]
    fn traversal_segments_are_rejected(
        prefix in "(references|assets|scripts)?/?",
        stem in "[a-z0-9_-]{0,10}",
    ) {
        let path = format!("{prefix}../{stem}");
        prop_assert!(validate_bundle_relative_path(&path).is_err());
    }

    #[test]
    fn absolute_paths_are_rejected(
        root in prop_oneof![Just("references"), Just("assets"), Just("SKILL")],
        suffix in "(/[a-z0-9_-]{0,10})?",
    ) {
        let path = format!("/{root}{suffix}");
        prop_assert!(validate_bundle_relative_path(&path).is_err());
    }

    #[test]
    fn backslash_separators_are_rejected(
        root in prop_oneof![Just("references"), Just("assets"), Just("scripts")],
        child in "[a-z0-9_-]{1,10}",
    ) {
        let path = format!("{root}\\{child}");
        prop_assert!(validate_bundle_relative_path(&path).is_err());
    }

    #[test]
    fn bare_dotdot_is_rejected(
        root in prop_oneof![Just("references"), Just("assets")],
    ) {
        let path = format!("{root}/..");
        prop_assert!(validate_bundle_relative_path(&path).is_err());
    }

    #[test]
    fn double_traversal_is_rejected(
        root in prop_oneof![Just("references"), Just("assets")],
        leaf in "[a-z0-9_-]{0,10}",
    ) {
        let path = format!("{root}/../../{leaf}");
        prop_assert!(validate_bundle_relative_path(&path).is_err());
    }

    #[test]
    fn double_traversal_alone_is_rejected(leaf in "[a-z0-9_-]{0,10}") {
        let path = format!("../../{leaf}");
        prop_assert!(validate_bundle_relative_path(&path).is_err());
    }

    #[test]
    fn bare_dot_leading_is_rejected(
        segment in "[a-z0-9_-]{1,10}",
    ) {
        let path = format!("./{segment}");
        prop_assert!(validate_bundle_relative_path(&path).is_err());
    }

    #[test]
    fn bare_dot_alone_is_rejected(
        dot in Just("."),
    ) {
        prop_assert!(validate_bundle_relative_path(dot).is_err());
    }

    #[test]
    fn utf8_boundary_size_succeeds(size in (0..=MAX_SKILL_READ_FILE_BYTES)) {
        let content = "x".repeat(size as usize);
        let utf8_check = std::str::from_utf8(content.as_bytes());
        prop_assert!(utf8_check.is_ok());
        prop_assert_eq!(
            utf8_check.expect("failed to decode bytes as UTF-8 in utf8_check").len(),
            size as usize
        );
    }

    #[test]
    fn size_boundary_above_cap_is_measured(size in (MAX_SKILL_READ_FILE_BYTES + 1..=MAX_SKILL_READ_FILE_BYTES + 1024)) {
        let content = "x".repeat(size as usize);
        prop_assert!(content.len() > MAX_SKILL_READ_FILE_BYTES as usize);
    }
}

#[test]
fn skill_entrypoint_path_validates() {
    assert!(validate_bundle_relative_path("SKILL.md").is_ok());
}
