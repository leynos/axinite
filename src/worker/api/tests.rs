//! Tests for the worker HTTP client and its shared API type conversions.

use axum::extract::{Path, State};
use axum::routing::{get, post};
use axum::{Json, Router};
use rstest::rstest;

use super::*;
use crate::testing::credentials::TEST_BEARER_TOKEN;
use uuid::Uuid;

#[test]
fn test_url_construction() {
    let client = WorkerHttpClient::new(
        "http://host.docker.internal:50051".to_string(),
        Uuid::nil(),
        TEST_BEARER_TOKEN.to_string(),
    );

    assert_eq!(
        client.url("llm/complete"),
        format!(
            "http://host.docker.internal:50051/worker/{}/llm/complete",
            Uuid::nil()
        )
    );
}

#[rstest]
#[case("stop", FinishReason::Stop)]
#[case("length", FinishReason::Length)]
#[case("tool_use", FinishReason::ToolUse)]
#[case("tool_calls", FinishReason::ToolUse)]
#[case("content_filter", FinishReason::ContentFilter)]
#[case("unknown", FinishReason::Unknown)]
fn test_parse_finish_reason(#[case] input: &str, #[case] expected: FinishReason) {
    assert_eq!(parse_finish_reason(input), expected);
}

#[test]
fn test_credentials_url_construction() {
    let client = WorkerHttpClient::new(
        "http://host.docker.internal:50051".to_string(),
        Uuid::nil(),
        TEST_BEARER_TOKEN.to_string(),
    );

    assert_eq!(
        client.url("credentials"),
        format!(
            "http://host.docker.internal:50051/worker/{}/credentials",
            Uuid::nil()
        )
    );
}

#[test]
fn test_job_description_deserialization() {
    let json = r#"{"title":"Test","description":"desc","project_dir":null}"#;
    let job: JobDescription = serde_json::from_str(json)
        .expect("failed to deserialize JobDescription in test_job_description_deserialization");
    assert_eq!(job.title, "Test");
    assert_eq!(job.description, "desc");
    assert!(job.project_dir.is_none());
}

#[test]
fn remote_tool_catalog_url_construction() {
    let client = WorkerHttpClient::new(
        "http://host.docker.internal:50051".to_string(),
        Uuid::nil(),
        TEST_BEARER_TOKEN.to_string(),
    );

    assert_eq!(
        client.url(REMOTE_TOOL_CATALOG_PATH),
        format!(
            "http://host.docker.internal:50051{}",
            REMOTE_TOOL_CATALOG_ROUTE.replace("{job_id}", &Uuid::nil().to_string())
        )
    );
}

#[derive(Clone)]
struct TestState;

async fn reject_catalog(
    State(_state): State<TestState>,
    Path(_job_id): Path<Uuid>,
) -> (axum::http::StatusCode, &'static str) {
    (axum::http::StatusCode::FORBIDDEN, "nope")
}

async fn reject_execute(
    State(_state): State<TestState>,
    Path(_job_id): Path<Uuid>,
    Json(_req): Json<RemoteToolExecutionRequest>,
) -> (axum::http::StatusCode, &'static str) {
    (axum::http::StatusCode::BAD_REQUEST, "bad params")
}

#[tokio::test]
async fn remote_tool_catalog_reports_non_success_statuses() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind listener");
    let addr = listener.local_addr().expect("listener addr");
    let router = Router::new()
        .route(REMOTE_TOOL_CATALOG_ROUTE, get(reject_catalog))
        .with_state(TestState);
    let server = tokio::spawn(async move {
        axum::serve(listener, router).await.expect("serve router");
    });

    let client = WorkerHttpClient::new(
        format!("http://{}", addr),
        Uuid::new_v4(),
        TEST_BEARER_TOKEN.to_string(),
    );

    let err = client
        .get_remote_tool_catalog()
        .await
        .expect_err("catalog fetch should fail on non-success status");
    assert!(err.to_string().contains("GET /tools/catalog returned 403"));

    server.abort();
    let _ = server.await;
}

#[tokio::test]
async fn remote_tool_execute_reports_non_success_statuses() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind listener");
    let addr = listener.local_addr().expect("listener addr");
    let router = Router::new()
        .route(REMOTE_TOOL_EXECUTE_ROUTE, post(reject_execute))
        .with_state(TestState);
    let server = tokio::spawn(async move {
        axum::serve(listener, router).await.expect("serve router");
    });

    let client = WorkerHttpClient::new(
        format!("http://{}", addr),
        Uuid::new_v4(),
        TEST_BEARER_TOKEN.to_string(),
    );

    let err = client
        .execute_remote_tool("github_search", &serde_json::json!({"query": 7}))
        .await
        .expect_err("remote-tool execute should fail on non-success status");
    assert!(
        err.to_string()
            .contains("Remote tool execution: orchestrator returned 400")
    );

    server.abort();
    let _ = server.await;
}
