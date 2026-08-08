//! Multipart install request tests for the Skills handler.

use std::sync::Arc;

use anyhow::Context as _;
use axum::body::Body;
use axum::http::{Request, StatusCode};
use rstest::rstest;
use tower::ServiceExt;

use super::helpers::*;

#[rstest]
#[tokio::test]
async fn upload_skill_bundle_preserves_references_and_assets(
    skills_api_fixture: anyhow::Result<SkillsApiFixture>,
) {
    let fixture = skills_api_fixture.expect("skills API fixture should build");
    let archive = build_bundle_archive(&[
        (
            "deploy-docs/SKILL.md",
            skill_markdown("deploy-docs").as_bytes(),
        ),
        ("deploy-docs/references/usage.md", b"# Usage\n"),
        ("deploy-docs/assets/logo.txt", b"logo"),
    ])
    .expect("test bundle archive should build");
    let (content_type, body) = multipart_file_body("bundle", "deploy-docs.skill", &archive)
        .expect("multipart body should build");

    let response = skills_router(Arc::clone(&fixture.state))
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
    let body = response_text(response)
        .await
        .expect("response body should be readable");
    let body: serde_json::Value = serde_json::from_str(&body).expect("JSON response expected");
    assert_eq!(body["success"], true);

    let installed = fixture.installed_root.join("deploy-docs");
    assert!(installed.join("SKILL.md").exists());
    assert!(installed.join("references/usage.md").exists());
    assert!(installed.join("assets/logo.txt").exists());
}

#[rstest]
#[tokio::test]
async fn upload_skill_bundle_accepts_case_insensitive_content_type(
    skills_api_fixture: anyhow::Result<SkillsApiFixture>,
) {
    let fixture = skills_api_fixture.expect("skills API fixture should build");
    let archive = build_bundle_archive(&[(
        "deploy-docs/SKILL.md",
        skill_markdown("deploy-docs").as_bytes(),
    )])
    .expect("test bundle archive should build");
    let (content_type, body) = multipart_file_body("bundle", "deploy-docs.skill", &archive)
        .expect("multipart body should build");
    let content_type = content_type.replacen("multipart/form-data", "Multipart/Form-Data", 1);

    let (status, body) = post_skill_bundle_install(Arc::clone(&fixture.state), content_type, body)
        .await
        .expect("skill bundle install request should complete");

    assert_eq!(status, StatusCode::OK);
    let body: serde_json::Value = serde_json::from_str(&body).expect("JSON response expected");
    assert_eq!(body["success"], true);
}

/// Send a multipart POST to the skills install endpoint and return the status
/// code and response body text.
///
/// Request construction and dispatch can fail, so the errors are propagated
/// for the test body to unwrap.
async fn post_skill_bundle_install(
    state: Arc<crate::channels::web::server::GatewayState>,
    content_type: String,
    body: Vec<u8>,
) -> anyhow::Result<(StatusCode, String)> {
    let request = Request::builder()
        .method("POST")
        .uri("/api/skills/install")
        .header("x-confirm-action", "true")
        .header("content-type", content_type)
        .body(Body::from(body))
        .context("request should build")?;
    let response = skills_router(state)
        .oneshot(request)
        .await
        .context("request should complete")?;
    let status = response.status();
    let body = response_text(response).await?;
    Ok((status, body))
}

/// Builds a multipart request body, and its content type, from a bundle
/// archive. Body construction is fallible, so the rejection cases below can
/// share one function-pointer shape.
type MultipartBodyBuilder = fn(Vec<u8>) -> std::io::Result<(String, Vec<u8>)>;

fn body_with_missing_filename(archive: Vec<u8>) -> std::io::Result<(String, Vec<u8>)> {
    multipart_body(&[MultipartPart::FileWithoutFilename {
        field_name: "bundle",
        bytes: &archive,
    }])
}

fn body_with_wrong_extension(archive: Vec<u8>) -> std::io::Result<(String, Vec<u8>)> {
    multipart_file_body("bundle", "deploy-docs.zip", &archive)
}

#[rstest]
#[case::missing_filename(
    body_with_missing_filename as MultipartBodyBuilder,
    "Uploaded skill bundle must include a filename ending with .skill",
)]
#[case::wrong_extension(
    body_with_wrong_extension as MultipartBodyBuilder,
    "Uploaded skill bundle filename must end with .skill",
)]
#[tokio::test]
async fn upload_skill_bundle_rejects_invalid_bundle_filename(
    skills_api_fixture: anyhow::Result<SkillsApiFixture>,
    #[case] make_body: MultipartBodyBuilder,
    #[case] expected_error: &'static str,
) {
    let fixture = skills_api_fixture.expect("skills API fixture should build");
    let archive = build_bundle_archive(&[(
        "deploy-docs/SKILL.md",
        skill_markdown("deploy-docs").as_bytes(),
    )])
    .expect("test bundle archive should build");
    let (content_type, body) = make_body(archive).expect("multipart body should build");

    let (status, body) = post_skill_bundle_install(Arc::clone(&fixture.state), content_type, body)
        .await
        .expect("skill bundle install request should complete");

    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(body.contains(expected_error), "body was: {body}");
}

#[rstest]
#[tokio::test]
async fn upload_skill_bundle_rejects_multiple_bundle_fields(
    skills_api_fixture: anyhow::Result<SkillsApiFixture>,
) {
    let fixture = skills_api_fixture.expect("skills API fixture should build");
    let archive = build_bundle_archive(&[(
        "deploy-docs/SKILL.md",
        skill_markdown("deploy-docs").as_bytes(),
    )])
    .expect("test bundle archive should build");
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
    ])
    .expect("multipart body should build");

    let (status, body) = post_skill_bundle_install(Arc::clone(&fixture.state), content_type, body)
        .await
        .expect("skill bundle install request should complete");

    assert_eq!(status, StatusCode::BAD_REQUEST);
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
    skills_api_fixture: anyhow::Result<SkillsApiFixture>,
    #[case] field_name: &str,
    #[case] value: &str,
) {
    let fixture = skills_api_fixture.expect("skills API fixture should build");
    let archive = build_bundle_archive(&[(
        "deploy-docs/SKILL.md",
        skill_markdown("deploy-docs").as_bytes(),
    )])
    .expect("test bundle archive should build");
    let (content_type, body) = multipart_body(&[
        MultipartPart::File {
            field_name: "bundle",
            file_name: "deploy-docs.skill",
            bytes: &archive,
        },
        MultipartPart::Text { field_name, value },
    ])
    .expect("multipart body should build");

    let response = skills_router(Arc::clone(&fixture.state))
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
    let body = response_text(response)
        .await
        .expect("response body should be readable");
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
    skills_api_fixture: anyhow::Result<SkillsApiFixture>,
    #[case] field_name: &str,
    #[case] value: &str,
) {
    let fixture = skills_api_fixture.expect("skills API fixture should build");
    let archive = build_bundle_archive(&[(
        "deploy-docs/SKILL.md",
        skill_markdown("deploy-docs").as_bytes(),
    )])
    .expect("test bundle archive should build");
    let (content_type, body) = multipart_body(&[
        MultipartPart::File {
            field_name: "bundle",
            file_name: "deploy-docs.skill",
            bytes: &archive,
        },
        MultipartPart::Text { field_name, value },
    ])
    .expect("multipart body should build");

    let response = skills_router(Arc::clone(&fixture.state))
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
async fn upload_skill_bundle_reports_archive_shape_errors(
    skills_api_fixture: anyhow::Result<SkillsApiFixture>,
) {
    let fixture = skills_api_fixture.expect("skills API fixture should build");
    let archive = build_bundle_archive(&[
        ("first/SKILL.md", skill_markdown("first").as_bytes()),
        ("second/SKILL.md", skill_markdown("second").as_bytes()),
    ])
    .expect("test bundle archive should build");
    let (content_type, body) = multipart_file_body("bundle", "broken.skill", &archive)
        .expect("multipart body should build");

    let (status, body) = post_skill_bundle_install(Arc::clone(&fixture.state), content_type, body)
        .await
        .expect("skill bundle install request should complete");

    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(body.contains("invalid_skill_bundle"), "body was: {body}");
    assert!(
        body.contains("expected one top-level path prefix"),
        "body was: {body}"
    );
}
