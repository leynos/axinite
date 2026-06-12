//! Property tests for skill location and bundle install invariants.

use std::collections::{BTreeMap, BTreeSet};
use std::io::Write;
use std::path::Path;
use std::path::PathBuf;

use proptest::prelude::*;

use crate::skills::registry::{SkillInstallPayload, SkillRegistry};
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

fn arb_bundle_entries() -> impl Strategy<Value = Vec<(String, Vec<u8>)>> {
    prop::collection::btree_set(arb_bundle_path(), 1..=8).prop_flat_map(|paths| {
        let paths = paths.into_iter().collect::<Vec<_>>();
        prop::collection::vec("[ -~\n]{0,1024}", paths.len())
            .prop_map(move |bodies| paths.iter().cloned().zip(bodies).collect())
            .prop_map(|entries: Vec<(String, String)>| {
                entries
                    .into_iter()
                    .map(|(path, body)| (path, body.into_bytes()))
                    .collect()
            })
    })
}

fn arb_bundle_path() -> impl Strategy<Value = String> {
    let id = "[a-z][a-z0-9_-]{0,7}";
    prop_oneof![
        id.prop_map(|name| format!("deploy-docs/references/{name}.md")),
        (id, id).prop_map(|(dir, name)| format!("deploy-docs/references/{dir}/{name}.md")),
        id.prop_map(|name| format!("deploy-docs/assets/{name}.txt")),
        id.prop_map(|name| format!("deploy-docs/assets/{name}.bin")),
    ]
}

fn skill_markdown(name: &str) -> Vec<u8> {
    format!("---\nname: {name}\n---\n\n# {name}\n").into_bytes()
}

fn build_stored_bundle_archive(entries: &[(String, Vec<u8>)]) -> Vec<u8> {
    let cursor = std::io::Cursor::new(Vec::new());
    let mut writer = zip::ZipWriter::new(cursor);
    let options =
        zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);

    for (name, contents) in entries {
        writer
            .start_file(name, options)
            .expect("test archive should start file");
        writer
            .write_all(contents)
            .expect("test archive should write file contents");
    }

    writer
        .finish()
        .expect("test archive should finish")
        .into_inner()
}

fn collect_installed_files(root: &Path) -> BTreeMap<PathBuf, Vec<u8>> {
    fn visit(base: &Path, current: &Path, files: &mut BTreeMap<PathBuf, Vec<u8>>) {
        for entry in std::fs::read_dir(current).expect("installed directory should be readable") {
            let entry = entry.expect("installed directory entry should be readable");
            let path = entry.path();
            if path.is_dir() {
                visit(base, &path, files);
            } else {
                let relative = path
                    .strip_prefix(base)
                    .expect("installed file should be under bundle root")
                    .to_path_buf();
                let contents = std::fs::read(&path).expect("installed file should be readable");
                files.insert(relative, contents);
            }
        }
    }

    let mut files = BTreeMap::new();
    visit(root, root, &mut files);
    files
}

proptest! {
    #![proptest_config(ProptestConfig {
        cases: 32,
        max_shrink_iters: 64,
        ..ProptestConfig::default()
    })]

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

    #[test]
    fn prop_bundle_round_trip_preserves_entries(generated_entries in arb_bundle_entries()) {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("test runtime should build");
        runtime.block_on(async move {
            let user_dir = tempfile::tempdir().expect("user tempdir should be created for test");
            let installed_dir = tempfile::tempdir().expect("installed tempdir should be created for test");
            let mut registry = SkillRegistry::new(user_dir.path().to_path_buf())
                .with_installed_dir(installed_dir.path().to_path_buf());

            let mut paths = generated_entries
                .iter()
                .map(|(path, _)| path.as_str())
                .collect::<BTreeSet<_>>();
            paths.insert("deploy-docs/SKILL.md");
            prop_assert_eq!(paths.len(), generated_entries.len() + 1);

            let mut entries = Vec::with_capacity(generated_entries.len() + 1);
            entries.push(("deploy-docs/SKILL.md".to_string(), skill_markdown("deploy-docs")));
            entries.extend(generated_entries);
            let archive = build_stored_bundle_archive(&entries);

            let prepared = SkillRegistry::prepare_install_to_disk(
                registry.install_target_dir(),
                SkillInstallPayload::ArchiveBytes(archive),
            )
            .await
            .expect("generated valid bundle should prepare");
            registry
                .commit_install(prepared)
                .expect("generated valid bundle should commit");

            let expected = entries
                .into_iter()
                .map(|(path, contents)| {
                    (
                        PathBuf::from(
                            path.strip_prefix("deploy-docs/")
                                .expect("generated path should be bundle rooted"),
                        ),
                        contents,
                    )
                })
                .collect::<BTreeMap<_, _>>();
            let installed_root = installed_dir.path().join("deploy-docs");
            prop_assert_eq!(collect_installed_files(&installed_root), expected);

            Ok(())
        })?;
    }
}
