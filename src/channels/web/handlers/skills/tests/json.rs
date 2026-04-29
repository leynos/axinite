//! JSON install request tests for the Skills handler.

use std::sync::Arc;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use rstest::rstest;
use tower::ServiceExt;

use super::helpers::*;
use crate::channels::web::handlers::install_helpers::MAX_SKILL_INSTALL_REQUEST_BYTES;

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
