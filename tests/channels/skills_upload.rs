//! End-to-end tests for skill bundle uploads through the web gateway.
//!
//! The malformed archive matrix includes traversal coverage for the zip-slip
//! class of bugs represented by CVE-2018-1002200. It also locks down
//! executable-extension rejection so passive skill bundles cannot grow an
//! accidental script-install surface.

use std::sync::{Arc, RwLock};

use ironclaw::channels::web::test_helpers::TestGatewayBuilder;
use ironclaw::skills::SkillRegistry;
use ironclaw::skills::test_support::{
    build_bundle_archive as try_build_bundle_archive,
    build_bundle_archive_from_owned as try_build_bundle_archive_from_owned,
};
#[cfg(target_os = "linux")]
use ironclaw::tools::NativeTool;
#[cfg(target_os = "linux")]
use ironclaw::tools::builtin::SkillReadFileTool;
use reqwest::multipart::{Form, Part};
use rstest::rstest;
use tempfile::TempDir;

const AUTH_TOKEN: &str = "test-skill-upload-token";
const DEPLOY_DOCS_SKILL_MD: &str = "---\nname: deploy-docs\ndescription: Deploy documentation\nversion: 1.0.0\n---\n# Deploy docs\n";
const PNG_SIGNATURE: &[u8] = &[0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A];

#[derive(Debug, Clone, Copy)]
enum MalformedKind {
    ScriptsDir,
    ExecutableExtensionUnderAssets,
    DuplicateCaseFold,
    Traversal,
    OversizedArchive,
    MissingSkillMd,
    MultipleTopLevelPrefixes,
}

fn build_bundle_archive(entries: &[(&str, &[u8])]) -> Vec<u8> {
    try_build_bundle_archive(entries).expect("test bundle archive should build")
}

fn build_bundle_archive_from_owned(entries: Vec<(String, Vec<u8>)>) -> Vec<u8> {
    try_build_bundle_archive_from_owned(entries).expect("test bundle archive should build")
}

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
                Part::bytes(build_bundle_archive(&documented_bundle_entries()))
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
            .join("deploy-docs/references/nested/api.md")
            .exists()
    );
    assert!(
        installed_dir
            .path()
            .join("deploy-docs/assets/note.txt")
            .exists()
    );
    assert!(
        installed_dir
            .path()
            .join("deploy-docs/assets/logo.png")
            .exists()
    );
}

#[rstest]
#[case::scripts_dir(MalformedKind::ScriptsDir)]
#[case::executable_extension_under_assets(MalformedKind::ExecutableExtensionUnderAssets)]
#[case::duplicate_case_fold(MalformedKind::DuplicateCaseFold)]
#[case::traversal(MalformedKind::Traversal)]
#[case::oversized_archive(MalformedKind::OversizedArchive)]
#[case::missing_skill_md(MalformedKind::MissingSkillMd)]
#[case::multiple_top_level_prefixes(MalformedKind::MultipleTopLevelPrefixes)]
#[tokio::test]
async fn multipart_skill_bundle_upload_rejects_malformed_bundles(#[case] kind: MalformedKind) {
    let (_user_dir, installed_dir, registry, addr) = start_skill_gateway().await;

    let response = upload_bundle(&addr, build_malformed_archive(kind), "deploy-docs.skill").await;
    let status = response.status();
    let body = response
        .text()
        .await
        .expect("error response body should be readable");

    assert_eq!(status, reqwest::StatusCode::BAD_REQUEST);
    assert!(
        body.contains("invalid_skill_bundle"),
        "expected invalid_skill_bundle body, got {body}"
    );
    assert!(
        install_dir_entries(installed_dir.path()).is_empty(),
        "malformed upload should not leave install entries"
    );
    assert!(
        !registry
            .read()
            .expect("registry lock should be readable")
            .has("deploy-docs"),
        "malformed upload should not register a skill"
    );
}

#[tokio::test]
async fn multipart_skill_bundle_upload_rejects_non_skill_filename() {
    let (_user_dir, installed_dir, _registry, addr) = start_skill_gateway().await;

    let response = upload_bundle(
        &addr,
        build_bundle_archive(&documented_bundle_entries()),
        "deploy-docs.zip",
    )
    .await;
    let status = response.status();
    let body = response
        .text()
        .await
        .expect("error response body should be readable");

    assert_eq!(status, reqwest::StatusCode::BAD_REQUEST);
    assert!(
        body.contains("filename must end with .skill"),
        "unexpected filename rejection body: {body}"
    );
    assert!(install_dir_entries(installed_dir.path()).is_empty());
}

#[rstest]
#[case::entrypoint(
    "SKILL.md",
    "text/markdown",
    "---\nname: deploy-docs\ndescription: Deploy documentation\nversion: 1.0.0\n---\n# Deploy docs\n"
)]
#[case::reference("references/usage.md", "text/markdown", "# Usage\n")]
#[case::nested_reference("references/nested/api.md", "text/markdown", "# API\n")]
#[case::text_asset("assets/note.txt", "text/plain", "asset notes\n")]
#[tokio::test]
#[cfg(target_os = "linux")]
async fn multipart_skill_bundle_upload_round_trip_reads_each_entry(
    #[case] path: &str,
    #[case] mime_type: &str,
    #[case] content: &str,
) {
    let (_user_dir, _installed_dir, registry, addr) = start_skill_gateway().await;

    let response = upload_bundle(
        &addr,
        build_bundle_archive(&documented_bundle_entries()),
        "deploy-docs.skill",
    )
    .await;
    assert_eq!(response.status(), reqwest::StatusCode::OK);

    let tool = SkillReadFileTool::new(Arc::clone(&registry));
    let output = NativeTool::execute(
        &tool,
        serde_json::json!({
            "skill": "deploy-docs",
            "path": path,
        }),
        &ironclaw::context::JobContext::default(),
    )
    .await
    .expect("skill_read_file should return uploaded entry");

    assert_eq!(output.result["skill"], "deploy-docs");
    assert_eq!(output.result["path"], path);
    assert_eq!(output.result["mime_type"], mime_type);
    assert_eq!(output.result["content"], content);
}

async fn start_skill_gateway() -> (
    TempDir,
    TempDir,
    Arc<RwLock<SkillRegistry>>,
    std::net::SocketAddr,
) {
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
    (user_dir, installed_dir, registry, addr)
}

async fn upload_bundle(
    addr: &std::net::SocketAddr,
    archive: Vec<u8>,
    filename: &'static str,
) -> reqwest::Response {
    reqwest::Client::new()
        .post(format!("http://{addr}/api/skills/install"))
        .bearer_auth(AUTH_TOKEN)
        .header("x-confirm-action", "true")
        .multipart(
            Form::new().part(
                "bundle",
                Part::bytes(archive)
                    .file_name(filename)
                    .mime_str("application/octet-stream")
                    .expect("bundle MIME type should parse"),
            ),
        )
        .send()
        .await
        .expect("upload request should complete")
}

fn documented_bundle_entries() -> Vec<(&'static str, &'static [u8])> {
    vec![
        ("deploy-docs/SKILL.md", DEPLOY_DOCS_SKILL_MD.as_bytes()),
        ("deploy-docs/references/usage.md", b"# Usage\n"),
        ("deploy-docs/references/nested/api.md", b"# API\n"),
        ("deploy-docs/assets/note.txt", b"asset notes\n"),
        ("deploy-docs/assets/logo.png", PNG_SIGNATURE),
    ]
}

fn build_malformed_archive(kind: MalformedKind) -> Vec<u8> {
    match kind {
        MalformedKind::ScriptsDir => build_bundle_archive(&[
            ("deploy-docs/SKILL.md", DEPLOY_DOCS_SKILL_MD.as_bytes()),
            ("deploy-docs/scripts/install.sh", b"echo nope"),
        ]),
        MalformedKind::ExecutableExtensionUnderAssets => build_bundle_archive(&[
            ("deploy-docs/SKILL.md", DEPLOY_DOCS_SKILL_MD.as_bytes()),
            ("deploy-docs/assets/install.sh", b"echo nope"),
        ]),
        MalformedKind::DuplicateCaseFold => build_bundle_archive(&[
            ("deploy-docs/SKILL.md", DEPLOY_DOCS_SKILL_MD.as_bytes()),
            ("deploy-docs/references/Guide.md", b"# A\n"),
            ("deploy-docs/references/guide.md", b"# B\n"),
        ]),
        MalformedKind::Traversal => build_bundle_archive(&[
            ("deploy-docs/SKILL.md", DEPLOY_DOCS_SKILL_MD.as_bytes()),
            ("deploy-docs/../evil.md", b"bad"),
        ]),
        MalformedKind::OversizedArchive => {
            let large = vec![b'x'; 512 * 1024 + 1];
            build_bundle_archive_from_owned(vec![
                (
                    "deploy-docs/SKILL.md".to_string(),
                    DEPLOY_DOCS_SKILL_MD.as_bytes().to_vec(),
                ),
                ("deploy-docs/assets/big.bin".to_string(), large),
            ])
        }
        MalformedKind::MissingSkillMd => {
            build_bundle_archive(&[("deploy-docs/references/usage.md", b"# Usage\n")])
        }
        MalformedKind::MultipleTopLevelPrefixes => build_bundle_archive(&[
            ("deploy-docs/SKILL.md", DEPLOY_DOCS_SKILL_MD.as_bytes()),
            ("other-skill/references/usage.md", b"# Usage\n"),
        ]),
    }
}

fn install_dir_entries(path: &std::path::Path) -> Vec<String> {
    std::fs::read_dir(path)
        .expect("install dir should be readable")
        .filter_map(Result::ok)
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .collect()
}
