//! Property tests for skill location invariants.

use std::path::PathBuf;

use proptest::prelude::*;

use crate::skills::{
    ActivationCriteria, LoadedSkill, LoadedSkillLocation, LoadedSkillParts, SkillManifest,
    SkillPackageKind, SkillSource, SkillTrust,
};

/// Arbitrary valid skill name matching `^[a-zA-Z0-9][a-zA-Z0-9._-]{0,63}$`.
fn arb_skill_name() -> impl Strategy<Value = String> {
    "[a-zA-Z0-9][a-zA-Z0-9._-]{0,20}"
}

/// Arbitrary relative path (no leading `/`).  Produces both flat
/// filenames and single-level nested paths such as `subdir/SKILL.md`.
fn arb_relative_path() -> impl Strategy<Value = PathBuf> {
    let flat = "[a-zA-Z0-9_-]{1,10}(\\.[a-zA-Z]{1,4})?".prop_map(PathBuf::from);
    let nested = ("[a-zA-Z0-9_-]{1,8}", "/SKILL\\.md")
        .prop_map(|(dir, file): (String, String)| PathBuf::from(format!("{dir}{file}")));
    prop_oneof![flat, nested]
}

proptest! {
    /// The manifest name and location identifier must always match after
    /// a successful `LoadedSkill::new`.
    #[test]
    fn prop_loaded_skill_identifier_matches_manifest(name in arb_skill_name()) {
        let location = LoadedSkillLocation::new(
            &name,
            PathBuf::from("/tmp"),
            PathBuf::from("SKILL.md"),
            SkillPackageKind::SingleFile,
        )
        .expect("test entrypoint is bundle-relative");
        let skill = LoadedSkill::new(LoadedSkillParts {
            manifest: SkillManifest {
                name: name.clone(),
                version: "1.0.0".to_string(),
                description: String::new(),
                activation: ActivationCriteria::default(),
                metadata: None,
            },
            prompt_content: String::new(),
            trust: SkillTrust::Trusted,
            source: SkillSource::User(PathBuf::from("/tmp")),
            location,
            content_hash: String::new(),
            compiled_patterns: vec![],
            lowercased_keywords: vec![],
            lowercased_exclude_keywords: vec![],
            lowercased_tags: vec![],
        }).expect("matching name and identifier should always succeed");
        prop_assert_eq!(skill.skill_identifier(), name.as_str());
        prop_assert_eq!(skill.manifest.name, name);
    }

    /// A mismatched identifier must always be rejected.
    #[test]
    fn prop_mismatched_identifier_always_rejected(
        manifest_name in arb_skill_name(),
        location_name in arb_skill_name(),
    ) {
        prop_assume!(manifest_name != location_name);
        let location = LoadedSkillLocation::new(
            &location_name,
            PathBuf::from("/tmp"),
            PathBuf::from("SKILL.md"),
            SkillPackageKind::SingleFile,
        )
        .expect("test entrypoint is bundle-relative");
        let result = LoadedSkill::new(LoadedSkillParts {
            manifest: SkillManifest {
                name: manifest_name,
                version: "1.0.0".to_string(),
                description: String::new(),
                activation: ActivationCriteria::default(),
                metadata: None,
            },
            prompt_content: String::new(),
            trust: SkillTrust::Trusted,
            source: SkillSource::User(PathBuf::from("/tmp")),
            location,
            content_hash: String::new(),
            compiled_patterns: vec![],
            lowercased_keywords: vec![],
            lowercased_exclude_keywords: vec![],
            lowercased_tags: vec![],
        });
        prop_assert!(result.is_err());
    }

    /// The bundle-relative root is always `.` regardless of the runtime root.
    #[test]
    fn prop_bundle_relative_root_is_always_dot(
        name in arb_skill_name(),
        entry in arb_relative_path(),
    ) {
        let location = LoadedSkillLocation::new(
            &name,
            PathBuf::from("/some/arbitrary/private/path"),
            entry,
            SkillPackageKind::Bundle,
        )
        .expect("test entrypoint is bundle-relative");
        prop_assert_eq!(location.bundle_relative_root(), std::path::Path::new("."));
    }
}
