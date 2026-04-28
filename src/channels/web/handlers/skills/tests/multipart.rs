//! Multipart install request tests for the Skills handler.

use std::sync::Arc;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use rstest::rstest;
use tower::ServiceExt;

use super::helpers::*;

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

/// Send a multipart POST to the skills install endpoint and return the status
/// code and response body text.
async fn post_skill_bundle_install(
    state: Arc<crate::channels::web::server::GatewayState>,
    content_type: String,
    body: Vec<u8>,
) -> (StatusCode, String) {
    let response = skills_router(state)
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
    let status = response.status();
    let body = response_text(response).await;
    (status, body)
}

#[rstest]
#[tokio::test]
async fn upload_skill_bundle_rejects_non_skill_filename(skills_api_fixture: SkillsApiFixture) {
    let archive = build_bundle_archive(&[(
        "deploy-docs/SKILL.md",
        skill_markdown("deploy-docs").as_bytes(),
    )]);
    let (content_type, body) = multipart_file_body("bundle", "deploy-docs.zip", &archive);

    let (status, body) =
        post_skill_bundle_install(Arc::clone(&skills_api_fixture.state), content_type, body).await;

    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(
        body.contains("Uploaded skill bundle filename must end with .skill"),
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

    let (status, body) =
        post_skill_bundle_install(Arc::clone(&skills_api_fixture.state), content_type, body).await;

    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(body.contains("invalid_skill_bundle"), "body was: {body}");
    assert!(
        body.contains("expected one top-level path prefix"),
        "body was: {body}"
    );
}
