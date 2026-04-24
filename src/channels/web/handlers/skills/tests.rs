//! Tests for the web Skills management handlers.

use std::io::Write;
use std::sync::Arc;

use axum::body::{Body, to_bytes};
use axum::http::{Request, StatusCode};
use axum::{Router, routing::post};
use rstest::{fixture, rstest};
use tower::ServiceExt;

use super::skills_install_handler;
use crate::channels::web::test_helpers::TestGatewayBuilder;
use crate::skills::registry::SkillRegistry;

struct SkillsApiFixture {
    _installed_dir: tempfile::TempDir,
    state: Arc<crate::channels::web::server::GatewayState>,
    installed_root: std::path::PathBuf,
}

#[fixture]
fn skills_api_fixture() -> SkillsApiFixture {
    let user_dir = tempfile::tempdir().expect("user tempdir should be created");
    let installed_dir = tempfile::tempdir().expect("installed tempdir should be created");
    let installed_root = installed_dir.path().to_path_buf();
    let registry = SkillRegistry::new(user_dir.path().to_path_buf())
        .with_installed_dir(installed_root.clone());
    let registry = Arc::new(std::sync::RwLock::new(registry));
    let state = TestGatewayBuilder::new().skill_registry(registry).build();

    SkillsApiFixture {
        _installed_dir: installed_dir,
        state,
        installed_root,
    }
}

fn skills_router(state: Arc<crate::channels::web::server::GatewayState>) -> Router {
    Router::new()
        .route("/api/skills/install", post(skills_install_handler))
        .with_state(state)
}

fn skill_markdown(name: &str) -> String {
    format!("---\nname: {name}\n---\n\n# {name}\n")
}

fn build_bundle_archive(entries: &[(&str, &[u8])]) -> Vec<u8> {
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

fn multipart_body(field_name: &str, file_name: &str, bytes: &[u8]) -> (String, Vec<u8>) {
    let boundary = "axinite-skill-boundary";
    let mut body = Vec::new();
    write!(
        body,
        "--{boundary}\r\nContent-Disposition: form-data; name=\"{field_name}\"; filename=\"{file_name}\"\r\nContent-Type: application/octet-stream\r\n\r\n"
    )
    .expect("multipart header should write");
    body.extend_from_slice(bytes);
    write!(body, "\r\n--{boundary}--\r\n").expect("multipart footer should write");
    (format!("multipart/form-data; boundary={boundary}"), body)
}

async fn response_text(response: axum::response::Response) -> String {
    let bytes = to_bytes(response.into_body(), 1024 * 1024)
        .await
        .expect("response body should be readable");
    String::from_utf8(bytes.to_vec()).expect("response body should be UTF-8")
}

#[rstest]
#[tokio::test]
async fn upload_skill_bundle_preserves_references_and_assets(skills_api_fixture: SkillsApiFixture) {
    let archive = build_bundle_archive(&[
        (
            "deploy-docs/SKILL.md",
            skill_markdown("deploy-docs").as_bytes(),
        ),
        ("deploy-docs/references/usage.md", b"# Usage\n"),
        ("deploy-docs/assets/logo.txt", b"logo"),
    ]);
    let (content_type, body) = multipart_body("bundle", "deploy-docs.skill", &archive);

    let response = skills_router(Arc::clone(&skills_api_fixture.state))
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/skills/install")
                .header("x-confirm-action", "true")
                .header("content-type", content_type)
                .body(Body::from(body))
                .expect("request should build"),
        )
        .await
        .expect("request should complete");

    assert_eq!(response.status(), StatusCode::OK);
    let body: serde_json::Value =
        serde_json::from_str(&response_text(response).await).expect("JSON response expected");
    assert_eq!(body["success"], true);

    let installed = skills_api_fixture.installed_root.join("deploy-docs");
    assert!(installed.join("SKILL.md").exists());
    assert!(installed.join("references/usage.md").exists());
    assert!(installed.join("assets/logo.txt").exists());
}

#[rstest]
#[tokio::test]
async fn upload_skill_bundle_reports_archive_shape_errors(skills_api_fixture: SkillsApiFixture) {
    let archive = build_bundle_archive(&[
        ("first/SKILL.md", skill_markdown("first").as_bytes()),
        ("second/SKILL.md", skill_markdown("second").as_bytes()),
    ]);
    let (content_type, body) = multipart_body("bundle", "broken.skill", &archive);

    let response = skills_router(Arc::clone(&skills_api_fixture.state))
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/skills/install")
                .header("x-confirm-action", "true")
                .header("content-type", content_type)
                .body(Body::from(body))
                .expect("request should build"),
        )
        .await
        .expect("request should complete");

    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    let body = response_text(response).await;
    assert!(body.contains("invalid_skill_bundle"), "body was: {body}");
    assert!(
        body.contains("expected one top-level path prefix"),
        "body was: {body}"
    );
}

#[rstest]
#[tokio::test]
async fn json_skill_install_rejects_multiple_sources(skills_api_fixture: SkillsApiFixture) {
    let response = skills_router(Arc::clone(&skills_api_fixture.state))
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/skills/install")
                .header("x-confirm-action", "true")
                .header("content-type", "application/json")
                .body(Body::from(
                    serde_json::json!({
                        "name": "deploy-docs",
                        "url": "https://example.com/deploy-docs.skill"
                    })
                    .to_string(),
                ))
                .expect("request should build"),
        )
        .await
        .expect("request should complete");

    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    let body = response_text(response).await;
    assert!(body.contains("Provide exactly one"), "body was: {body}");
}

#[rstest]
#[tokio::test]
async fn json_skill_install_keeps_inline_content_flow(skills_api_fixture: SkillsApiFixture) {
    let response = skills_router(Arc::clone(&skills_api_fixture.state))
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/skills/install")
                .header("x-confirm-action", "true")
                .header("content-type", "application/json")
                .body(Body::from(
                    serde_json::json!({
                        "content": skill_markdown("inline-docs")
                    })
                    .to_string(),
                ))
                .expect("request should build"),
        )
        .await
        .expect("request should complete");

    assert_eq!(response.status(), StatusCode::OK);
    let body: serde_json::Value =
        serde_json::from_str(&response_text(response).await).expect("JSON response expected");
    assert_eq!(body["success"], true);
    assert!(
        skills_api_fixture
            .installed_root
            .join("inline-docs/SKILL.md")
            .exists()
    );
}
