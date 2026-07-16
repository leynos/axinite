//! Deterministic regression tests for skill bundle installation correctness
//! and byte-for-byte file preservation across supported install transports.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

mod lifecycle;
mod payloads;

fn assert_deploy_docs_bundle_files_present(path: &Path) {
    assert!(path.join("SKILL.md").exists());
    assert!(path.join("references/usage.md").exists());
    assert!(path.join("assets/logo.txt").exists());
}

fn documented_bundle_entries() -> Vec<(&'static str, &'static [u8])> {
    vec![
        (
            "deploy-docs/SKILL.md",
            b"---\nname: deploy-docs\n---\n\n# deploy-docs\n",
        ),
        ("deploy-docs/references/usage.md", b"# Usage\n"),
        ("deploy-docs/references/nested/api.md", b"# API\n"),
        ("deploy-docs/assets/note.txt", b"asset notes\n"),
        (
            "deploy-docs/assets/logo.png",
            &[0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A],
        ),
    ]
}

fn collect_installed_files(root: &Path) -> BTreeMap<PathBuf, Vec<u8>> {
    fn visit(base: &Path, current: &Path, files: &mut BTreeMap<PathBuf, Vec<u8>>) {
        for entry in ambient_fs::read_dir(current).expect("installed directory should be readable")
        {
            let entry = entry.expect("installed directory entry should be readable");
            let path = entry.path();
            if path.is_dir() {
                visit(base, &path, files);
            } else {
                let relative = path
                    .strip_prefix(base)
                    .expect("installed file should be under bundle root")
                    .to_path_buf();
                let contents = ambient_fs::read(&path).expect("installed file should be readable");
                files.insert(relative, contents);
            }
        }
    }

    let mut files = BTreeMap::new();
    visit(root, root, &mut files);
    files
}
