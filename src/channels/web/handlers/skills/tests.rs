//! Tests for the web Skills management handlers.

use std::io::Write;
use std::sync::Arc;

use axum::body::{Body, to_bytes};
use axum::http::{Request, StatusCode};
use axum::{Router, routing::post};
use rstest::{fixture, rstest};
use tower::ServiceExt;

use super::skills_install_handler;
use crate::channels::web::handlers::install_helpers::MAX_SKILL_INSTALL_REQUEST_BYTES;
use crate::channels::web::test_helpers::TestGatewayBuilder;
use crate::skills::registry::SkillRegistry;

struct SkillsApiFixture {
    _installed_dir: tempfile::TempDir,
    _user_dir: tempfile::TempDir,
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
        _user_dir: user_dir,
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

enum MultipartPart<'a> {
    File {
        field_name: &'a str,
        file_name: &'a str,
        bytes: &'a [u8],
    },
    FileWithoutFilename {
        field_name: &'a str,
        bytes: &'a [u8],
    },
    Text {
        field_name: &'a str,
        value: &'a str,
    },
}

fn multipart_file_body(field_name: &str, file_name: &str, bytes: &[u8]) -> (String, Vec<u8>) {
    multipart_body(&[MultipartPart::File {
        field_name,
        file_name,
        bytes,
    }])
}

fn multipart_body(parts: &[MultipartPart<'_>]) -> (String, Vec<u8>) {
    let boundary = "axinite-skill-boundary";
    let mut body = Vec::new();

    for part in parts {
        match part {
            MultipartPart::File {
                field_name,
                file_name,
                bytes,
            } => {
                write!(
                    body,
                    "--{boundary}\r\nContent-Disposition: form-data; name=\"{field_name}\"; filename=\"{file_name}\"\r\nContent-Type: application/octet-stream\r\n\r\n"
                )
                .expect("multipart file header should write");
                body.extend_from_slice(bytes);
            }
            MultipartPart::FileWithoutFilename { field_name, bytes } => {
                write!(
                    body,
                    "--{boundary}\r\nContent-Disposition: form-data; name=\"{field_name}\"\r\nContent-Type: application/octet-stream\r\n\r\n"
                )
                .expect("multipart file header should write");
                body.extend_from_slice(bytes);
            }
            MultipartPart::Text { field_name, value } => {
                write!(
                    body,
                    "--{boundary}\r\nContent-Disposition: form-data; name=\"{field_name}\"\r\n\r\n{value}"
                )
                .expect("multipart text field should write");
            }
        }
        write!(body, "\r\n").expect("multipart separator should write");
    }

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
    let (content_type, body) = multipart_file_body("bundle", "deploy-docs.skill", &archive);

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
async fn upload_skill_bundle_accepts_case_insensitive_content_type(
    skills_api_fixture: SkillsApiFixture,
) {
    let archive = build_bundle_archive(&[(
        "deploy-docs/SKILL.md",
        skill_markdown("deploy-docs").as_bytes(),
    )]);
    let (content_type, body) = multipart_file_body("bundle", "deploy-docs.skill", &archive);
    let content_type = content_type.replacen("multipart/form-data", "Multipart/Form-Data", 1);

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
}

#[rstest]
#[tokio::test]
async fn upload_skill_bundle_rejects_missing_filename(skills_api_fixture: SkillsApiFixture) {
    let archive = build_bundle_archive(&[(
        "deploy-docs/SKILL.md",
        skill_markdown("deploy-docs").as_bytes(),
    )]);
    let (content_type, body) = multipart_body(&[MultipartPart::FileWithoutFilename {
        field_name: "bundle",
        bytes: &archive,
    }]);

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
    assert!(
        body.contains("Uploaded skill bundle must include a filename ending with .skill"),
        "body was: {body}"
    );
}

#[rstest]
#[tokio::test]
async fn upload_skill_bundle_rejects_non_skill_filename(skills_api_fixture: SkillsApiFixture) {
    let archive = build_bundle_archive(&[(
        "deploy-docs/SKILL.md",
        skill_markdown("deploy-docs").as_bytes(),
    )]);
    let (content_type, body) = multipart_file_body("bundle", "deploy-docs.zip", &archive);

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
    assert!(
        body.contains("Uploaded skill bundle must include a filename ending with .skill"),
        "body was: {body}"
    );
}

#[rstest]
#[tokio::test]
async fn upload_skill_bundle_rejects_multiple_bundle_fields(skills_api_fixture: SkillsApiFixture) {
    let archive = build_bundle_archive(&[(
        "deploy-docs/SKILL.md",
        skill_markdown("deploy-docs").as_bytes(),
    )]);
    let (content_type, body) = multipart_body(&[
        MultipartPart::File {
            field_name: "bundle",
            file_name: "first.skill",
            bytes: &archive,
        },
        MultipartPart::File {
            field_name: "bundle",
            file_name: "second.skill",
            bytes: &archive,
        },
    ]);

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
    assert!(
        body.contains("Provide exactly one .skill upload"),
        "body was: {body}"
    );
}

#[rstest]
#[case::content("content", "inline skill content")]
#[case::url("url", "https://example.com/deploy-docs.skill")]
#[case::name("name", "deploy-docs")]
#[case::slug("slug", "owner/deploy-docs")]
#[tokio::test]
async fn upload_skill_bundle_rejects_additional_source_fields(
    skills_api_fixture: SkillsApiFixture,
    #[case] field_name: &str,
    #[case] value: &str,
) {
    let archive = build_bundle_archive(&[(
        "deploy-docs/SKILL.md",
        skill_markdown("deploy-docs").as_bytes(),
    )]);
    let (content_type, body) = multipart_body(&[
        MultipartPart::File {
            field_name: "bundle",
            file_name: "deploy-docs.skill",
            bytes: &archive,
        },
        MultipartPart::Text { field_name, value },
    ]);

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
    assert!(
        body.contains("Provide exactly one of 'content', 'url', 'name'/'slug', or a .skill upload"),
        "body was: {body}"
    );
}

#[rstest]
#[case::content("content", "   \t  ")]
#[case::url("url", "   \t  ")]
#[case::name("name", "   \t  ")]
#[case::slug("slug", "   \t  ")]
#[tokio::test]
async fn upload_skill_bundle_ignores_whitespace_only_source_fields(
    skills_api_fixture: SkillsApiFixture,
    #[case] field_name: &str,
    #[case] value: &str,
) {
    let archive = build_bundle_archive(&[(
        "deploy-docs/SKILL.md",
        skill_markdown("deploy-docs").as_bytes(),
    )]);
    let (content_type, body) = multipart_body(&[
        MultipartPart::File {
            field_name: "bundle",
            file_name: "deploy-docs.skill",
            bytes: &archive,
        },
        MultipartPart::Text { field_name, value },
    ]);

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
}

#[rstest]
#[tokio::test]
async fn upload_skill_bundle_reports_archive_shape_errors(skills_api_fixture: SkillsApiFixture) {
    let archive = build_bundle_archive(&[
        ("first/SKILL.md", skill_markdown("first").as_bytes()),
        ("second/SKILL.md", skill_markdown("second").as_bytes()),
    ]);
    let (content_type, body) = multipart_file_body("bundle", "broken.skill", &archive);

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

#[rstest]
#[tokio::test]
async fn json_skill_install_respects_max_request_size(skills_api_fixture: SkillsApiFixture) {
    let oversized_content = "a".repeat(MAX_SKILL_INSTALL_REQUEST_BYTES + 1);
    let body = serde_json::json!({
        "content": oversized_content,
    });

    let response = skills_router(Arc::clone(&skills_api_fixture.state))
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/skills/install")
                .header("x-confirm-action", "true")
                .header("content-type", "application/json")
                .body(Body::from(
                    serde_json::to_vec(&body).expect("body should serialize"),
                ))
                .expect("request should build"),
        )
        .await
        .expect("request should complete");

    assert_eq!(response.status(), StatusCode::PAYLOAD_TOO_LARGE);
    let body = response_text(response).await;
    assert!(
        body.contains("Request body exceeds maximum size of 10485760 bytes"),
        "body was: {body}"
    );
}
