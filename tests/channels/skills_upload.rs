//! End-to-end tests for skill bundle uploads through the web gateway.

use std::io::Write;
use std::sync::{Arc, RwLock};

use ironclaw::channels::web::test_helpers::TestGatewayBuilder;
use ironclaw::skills::SkillRegistry;
use reqwest::multipart::{Form, Part};
use tempfile::TempDir;
use zip::write::SimpleFileOptions;

const AUTH_TOKEN: &str = "test-skill-upload-token";

#[tokio::test]
async fn multipart_skill_bundle_upload_installs_bundle_files() {
    let user_dir = TempDir::new().expect("user dir should be created");
    let installed_dir = TempDir::new().expect("installed dir should be created");
    let registry = Arc::new(RwLock::new(
        SkillRegistry::new(user_dir.path().to_path_buf())
            .with_installed_dir(installed_dir.path().to_path_buf()),
    ));
    let (addr, _state) = TestGatewayBuilder::new()
        .skill_registry(Arc::clone(&registry))
        .start(AUTH_TOKEN)
        .await
        .expect("gateway should start");

    let response = reqwest::Client::new()
        .post(format!("http://{addr}/api/skills/install"))
        .bearer_auth(AUTH_TOKEN)
        .header("x-confirm-action", "true")
        .multipart(
            Form::new().part(
                "bundle",
                Part::bytes(build_bundle_archive())
                    .file_name("deploy-docs.skill")
                    .mime_str("application/octet-stream")
                    .expect("bundle MIME type should parse"),
            ),
        )
        .send()
        .await
        .expect("upload request should complete");

    assert_eq!(response.status(), reqwest::StatusCode::OK);
    assert!(
        registry
            .read()
            .expect("skill registry lock should be readable")
            .has("deploy-docs")
    );
    assert!(installed_dir.path().join("deploy-docs/SKILL.md").exists());
    assert!(
        installed_dir
            .path()
            .join("deploy-docs/references/usage.md")
            .exists()
    );
    assert!(
        installed_dir
            .path()
            .join("deploy-docs/assets/logo.txt")
            .exists()
    );
}

fn build_bundle_archive() -> Vec<u8> {
    let mut cursor = std::io::Cursor::new(Vec::new());
    {
        let mut archive = zip::ZipWriter::new(&mut cursor);
        let options = SimpleFileOptions::default();
        for (path, contents) in [
            (
                "deploy-docs/SKILL.md",
                skill_markdown("deploy-docs").as_bytes(),
            ),
            ("deploy-docs/references/usage.md", b"# Usage\n".as_slice()),
            ("deploy-docs/assets/logo.txt", b"logo".as_slice()),
        ] {
            archive
                .start_file(path, options)
                .expect("archive file should start");
            archive
                .write_all(contents)
                .expect("archive file should write");
        }
        archive.finish().expect("archive should finish");
    }
    cursor.into_inner()
}

fn skill_markdown(name: &str) -> String {
    format!(
        concat!(
            "---\n",
            "name: {name}\n",
            "description: Deploy documentation\n",
            "version: 1.0.0\n",
            "---\n",
            "# Deploy docs\n"
        ),
        name = name
    )
}
