//! Shared test fixtures and write helpers for the [`SkillRegistry`] test suite.
//!
//! Provides:
//! - [`BundleInstallFixture`] — registry pre-configured with both a user
//!   directory and an installed directory, used for bundle install tests.
//! - [`FreshRegistryFixture`] — lightweight registry backed by a single temp
//!   directory, used for discovery and lookup tests.
//! - [`bundle_install_fixture`] / [`fresh_registry_fixture`] — `rstest`
//!   `#[fixture]` constructors for the above.
//! - [`skill_markdown`] — generates minimal valid `SKILL.md` content.
//! - [`write_skill_subdir`] / [`write_skill_flat`] — write `SKILL.md` into
//!   a temp directory in subdirectory or flat layout respectively.
use std::path::Path;

use rstest::fixture;

use crate::skills::registry::SkillRegistry;

pub(super) struct BundleInstallFixture {
    pub(super) user_dir: tempfile::TempDir,
    pub(super) installed_dir: tempfile::TempDir,
    pub(super) registry: SkillRegistry,
}

pub(super) struct FreshRegistryFixture {
    pub(super) dir: tempfile::TempDir,
    pub(super) registry: SkillRegistry,
}

pub(super) fn skill_markdown(name: &str) -> String {
    format!("---\nname: {name}\n---\n\n# {name}\n")
}

pub(super) fn build_bundle_archive(entries: &[(&str, &[u8])]) -> Vec<u8> {
    crate::skills::test_support::build_bundle_archive(entries)
        .expect("test bundle archive should build")
}

#[fixture]
pub(super) fn bundle_install_fixture() -> BundleInstallFixture {
    let user_dir = tempfile::tempdir().expect("user tempdir should be created for test");
    let installed_dir = tempfile::tempdir().expect("installed tempdir should be created for test");
    let registry = SkillRegistry::new(user_dir.path().to_path_buf())
        .with_installed_dir(installed_dir.path().to_path_buf());

    BundleInstallFixture {
        user_dir,
        installed_dir,
        registry,
    }
}

#[fixture]
pub(super) fn fresh_registry_fixture() -> FreshRegistryFixture {
    let dir = tempfile::tempdir().expect("temp dir should be created for test");
    let registry = SkillRegistry::new(dir.path().to_path_buf());
    FreshRegistryFixture { dir, registry }
}

/// Writes `content` to `<root>/<skill_name>/SKILL.md`, creating the subdirectory.
pub(super) fn write_skill_subdir(root: &Path, skill_name: &str, content: &str) {
    let skill_dir = root.join(skill_name);
    std::fs::create_dir(&skill_dir).expect("skill subdirectory should be created for test");
    std::fs::write(skill_dir.join("SKILL.md"), content)
        .expect("SKILL.md should be written for test");
}

/// Writes `content` to `<root>/SKILL.md` (flat layout).
pub(super) fn write_skill_flat(root: &Path, content: &str) {
    std::fs::write(root.join("SKILL.md"), content)
        .expect("flat SKILL.md should be written for test");
}
