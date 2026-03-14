//! Tests for the worker HTTP client and its shared API type conversions.

use rstest::rstest;

use super::*;
use crate::llm::FinishReason as LlmFinishReason;
use crate::testing::credentials::TEST_BEARER_TOKEN;
use serde_json::json;
use uuid::Uuid;

#[rstest]
#[case("llm/complete")]
#[case("credentials")]
use axum::extract::{Path, State};
use axum::routing::{get, post};
use axum::{Json, Router};
fn test_url_construction(#[case] path: &str) {
    let client = WorkerHttpClient::new(
        "http://host.docker.internal:50051".to_string(),
        Uuid::nil(),
        TEST_BEARER_TOKEN.to_string(),
    );

    assert_eq!(
        client.url(path),
        format!(
            "http://host.docker.internal:50051/worker/{}/{}",
            Uuid::nil(),
            path
        )
    );
}

#[rstest]
#[case(json!("stop"), ProxyFinishReason::Stop)]
#[case(json!("length"), ProxyFinishReason::Length)]
#[case(json!("tool_use"), ProxyFinishReason::ToolUse)]
#[case(json!("tool_calls"), ProxyFinishReason::ToolUse)]
#[case(json!("content_filter"), ProxyFinishReason::ContentFilter)]
#[case(json!("unknown"), ProxyFinishReason::Unknown)]
fn test_proxy_finish_reason_deserialization(
    #[case] input: serde_json::Value,
    #[case] expected: ProxyFinishReason,
) {
    let actual: ProxyFinishReason = serde_json::from_value(input).expect(
        "failed to deserialize ProxyFinishReason in test_proxy_finish_reason_deserialization",
    );
    assert_eq!(actual, expected);
}

#[test]
fn test_proxy_finish_reason_unknown_provider_value_falls_back() {
    let reason = serde_json::from_value::<ProxyFinishReason>(json!("made_up_reason"))
        .expect("failed to deserialize unknown ProxyFinishReason as fallback");
    assert_eq!(reason, ProxyFinishReason::Unknown);
}

#[rstest]
#[case(ProxyFinishReason::Stop, LlmFinishReason::Stop)]
#[case(ProxyFinishReason::Length, LlmFinishReason::Length)]
#[case(ProxyFinishReason::ToolUse, LlmFinishReason::ToolUse)]
#[case(ProxyFinishReason::ContentFilter, LlmFinishReason::ContentFilter)]
#[case(ProxyFinishReason::Unknown, LlmFinishReason::Unknown)]
fn test_proxy_finish_reason_conversion(
    #[case] input: ProxyFinishReason,
    #[case] expected: LlmFinishReason,
) {
    assert_eq!(LlmFinishReason::from(input), expected);
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

#[test]
fn test_status_update_new_serializes_worker_state() {
    let update = StatusUpdate::new(WorkerState::InProgress, Some("Iteration 1".to_string()), 1);
    let value = serde_json::to_value(update).expect(
        "failed to serialize StatusUpdate in test_status_update_new_serializes_worker_state",
    );

    assert_eq!(value["state"], "in_progress");
    assert_eq!(value["message"], "Iteration 1");
    assert_eq!(value["iteration"], 1);
}

#[test]
fn test_status_update_deserializes_worker_state() {
    let update: StatusUpdate = serde_json::from_value(json!({
        "state": "failed",
        "message": "boom",
        "iteration": 7
    }))
    .expect("failed to deserialize StatusUpdate in test_status_update_deserializes_worker_state");

    assert_eq!(update.state, WorkerState::Failed);
    assert_eq!(update.message.as_deref(), Some("boom"));
    assert_eq!(update.iteration, 7);
}

async fn reject_catalog(
    State(_state): State<TestState>,
    Path(_job_id): Path<Uuid>,
) -> (axum::http::StatusCode, &'static str) {
    (axum::http::StatusCode::FORBIDDEN, "nope")
}

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

struct TestState;

async fn reject_execute(
    State(_state): State<TestState>,
    Path(_job_id): Path<Uuid>,
    Json(_req): Json<RemoteToolExecutionRequest>,
) -> (axum::http::StatusCode, &'static str) {
    (axum::http::StatusCode::BAD_REQUEST, "bad params")
}

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
