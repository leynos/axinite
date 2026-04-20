use std::io::Write;

use rstest::fixture;

use crate::skills::registry::SkillRegistry;

pub(super) struct BundleInstallFixture {
    pub(super) user_dir: tempfile::TempDir,
    pub(super) installed_dir: tempfile::TempDir,
    pub(super) registry: SkillRegistry,
}

pub(super) fn skill_markdown(name: &str) -> String {
    format!("---\nname: {name}\n---\n\n# {name}\n")
}

pub(super) fn build_bundle_archive(entries: &[(&str, &[u8])]) -> Vec<u8> {
    let cursor = std::io::Cursor::new(Vec::new());
    let mut writer = zip::ZipWriter::new(cursor);
    let options = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated);

    for (name, contents) in entries {
        writer
            .start_file(*name, options)
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
