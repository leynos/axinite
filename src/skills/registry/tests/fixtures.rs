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
//!
//! Arrangement can fail, so every helper here that touches the filesystem is
//! fallible.  Callers in test bodies unwrap the returned `Result`; a failure
//! there is the test verdict.
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

/// Builds a `.skill` archive from `entries`, propagating archive failures.
pub(super) fn build_bundle_archive(
    entries: &[(&str, &[u8])],
) -> Result<Vec<u8>, zip::result::ZipError> {
    crate::skills::test_support::build_bundle_archive(entries)
}

/// Builds a registry with both a user and an installed directory.
///
/// Returns an error if either temporary directory cannot be created.
#[fixture]
pub(super) fn bundle_install_fixture() -> std::io::Result<BundleInstallFixture> {
    let user_dir = tempfile::tempdir()?;
    let installed_dir = tempfile::tempdir()?;
    let registry = SkillRegistry::new(user_dir.path().to_path_buf())
        .with_installed_dir(installed_dir.path().to_path_buf());

    Ok(BundleInstallFixture {
        user_dir,
        installed_dir,
        registry,
    })
}

/// Builds a registry backed by a single temporary directory.
///
/// Returns an error if the temporary directory cannot be created.
#[fixture]
pub(super) fn fresh_registry_fixture() -> std::io::Result<FreshRegistryFixture> {
    let dir = tempfile::tempdir()?;
    let registry = SkillRegistry::new(dir.path().to_path_buf());
    Ok(FreshRegistryFixture { dir, registry })
}

/// Writes `content` to `<root>/<skill_name>/SKILL.md`, creating the subdirectory.
///
/// Returns an error if the subdirectory or the file cannot be written.
pub(super) fn write_skill_subdir(
    root: &Path,
    skill_name: &str,
    content: &str,
) -> std::io::Result<()> {
    let skill_dir = root.join(skill_name);
    ambient_fs::create_dir(&skill_dir)?;
    ambient_fs::write(skill_dir.join("SKILL.md"), content)
}

/// Writes `content` to `<root>/SKILL.md` (flat layout).
///
/// Returns an error if the file cannot be written.
pub(super) fn write_skill_flat(root: &Path, content: &str) -> std::io::Result<()> {
    ambient_fs::write(root.join("SKILL.md"), content)
}
