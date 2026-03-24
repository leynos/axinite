//! Tests for the worker HTTP client and its shared API type conversions.

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::routing::{get, post};
use axum::{Json, Router};
use rstest::{fixture, rstest};
use std::future::Future;
use std::pin::Pin;

use super::*;
use crate::error::WorkerError;
use crate::llm::FinishReason as LlmFinishReason;
use crate::testing::credentials::TEST_BEARER_TOKEN;
use serde_json::json;
use uuid::Uuid;

#[rstest]
#[case("llm/complete")]
#[case("credentials")]
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

#[rstest]
#[tokio::test]
async fn remote_tool_catalog_reports_non_success_statuses(
    remote_tool_failure_server: RemoteToolFailureServerFactory,
) {
    let server = remote_tool_failure_server(RemoteToolFailureRoute::Catalog).await;

    let client = WorkerHttpClient::new(
        server.base_url,
        Uuid::new_v4(),
        TEST_BEARER_TOKEN.to_string(),
    );

    let err = client
        .get_remote_tool_catalog()
        .await
        .expect_err("catalog fetch should fail on non-success status");

    match err {
        WorkerError::OrchestratorRejected { reason, .. } => {
            assert!(reason.contains("GET /tools/catalog returned 403"));
        }
        other => panic!("unexpected worker error: {other:?}"),
    };

    server.handle.abort();
    let _ = server.handle.await;
}

#[derive(Clone, Copy)]
struct TestState;

async fn reject_execute(
    State(_state): State<TestState>,
    Path(_job_id): Path<Uuid>,
    Json(_req): Json<RemoteToolExecutionRequest>,
) -> (StatusCode, &'static str) {
    (StatusCode::BAD_REQUEST, "bad params")
}

async fn reject_execute_forbidden(
    State(_state): State<TestState>,
    Path(_job_id): Path<Uuid>,
    Json(_req): Json<RemoteToolExecutionRequest>,
) -> (StatusCode, &'static str) {
    (StatusCode::FORBIDDEN, "approval required")
}

#[rstest]
#[case(RemoteToolFailureRoute::ExecuteBadRequest, "bad params")]
#[case(RemoteToolFailureRoute::ExecuteForbidden, "approval required")]
#[case(RemoteToolFailureRoute::ExecuteRateLimited, "slow down")]
#[case(RemoteToolFailureRoute::ExecuteBadGateway, "proxy failure")]
#[case(RemoteToolFailureRoute::ExecuteInternalError, "remote tool blew up")]
#[tokio::test]
async fn remote_tool_execute_preserves_non_success_statuses(
    remote_tool_failure_server: RemoteToolFailureServerFactory,
    #[case] route: RemoteToolFailureRoute,
    #[case] expected_message: &str,
) {
    let server = remote_tool_failure_server(route).await;

    let client = WorkerHttpClient::new(
        server.base_url,
        Uuid::new_v4(),
        TEST_BEARER_TOKEN.to_string(),
    );

    let err = client
        .execute_remote_tool("github_search", &serde_json::json!({"query": 7}))
        .await
        .expect_err("remote-tool execute should fail on non-success status");

    match (route, err) {
        (RemoteToolFailureRoute::ExecuteBadRequest, WorkerError::BadRequest { reason }) => {
            assert!(reason.contains(expected_message))
        }
        (RemoteToolFailureRoute::ExecuteForbidden, WorkerError::Unauthorized { reason }) => {
            assert!(reason.contains(expected_message))
        }
        (
            RemoteToolFailureRoute::ExecuteRateLimited,
            WorkerError::RateLimited {
                reason,
                retry_after: Some(retry_after),
            },
        ) => {
            assert!(reason.contains(expected_message));
            assert_eq!(retry_after, std::time::Duration::from_secs(7));
        }
        (RemoteToolFailureRoute::ExecuteBadGateway, WorkerError::BadGateway { reason }) => {
            assert!(reason.contains(expected_message))
        }
        (
            RemoteToolFailureRoute::ExecuteInternalError,
            WorkerError::RemoteToolFailed { reason },
        ) => {
            assert!(reason.contains(expected_message))
        }
        (_, other) => panic!("unexpected worker error: {other:?}"),
    }

    server.handle.abort();
    let _ = server.handle.await;
}

type RemoteToolFailureServerFuture = Pin<Box<dyn Future<Output = RemoteToolFailureServer> + Send>>;

struct RemoteToolFailureServer {
    base_url: String,
    handle: tokio::task::JoinHandle<()>,
}

#[fixture]
fn remote_tool_failure_server() -> RemoteToolFailureServerFactory {
    Box::new(|route| {
        Box::pin(async move {
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
                .await
                .expect("bind listener");
            let addr = listener.local_addr().expect("listener addr");
            let router = match route {
                RemoteToolFailureRoute::Catalog => Router::new()
                    .route(REMOTE_TOOL_CATALOG_ROUTE, get(reject_catalog))
                    .with_state(TestState),
                RemoteToolFailureRoute::ExecuteBadRequest => Router::new()
                    .route(REMOTE_TOOL_EXECUTE_ROUTE, post(reject_execute))
                    .with_state(TestState),
                RemoteToolFailureRoute::ExecuteForbidden => Router::new()
                    .route(REMOTE_TOOL_EXECUTE_ROUTE, post(reject_execute_forbidden))
                    .with_state(TestState),
                RemoteToolFailureRoute::ExecuteRateLimited => Router::new()
                    .route(REMOTE_TOOL_EXECUTE_ROUTE, post(reject_execute_rate_limited))
                    .with_state(TestState),
                RemoteToolFailureRoute::ExecuteBadGateway => Router::new()
                    .route(REMOTE_TOOL_EXECUTE_ROUTE, post(reject_execute_bad_gateway))
                    .with_state(TestState),
                RemoteToolFailureRoute::ExecuteInternalError => Router::new()
                    .route(
                        REMOTE_TOOL_EXECUTE_ROUTE,
                        post(reject_execute_internal_error),
                    )
                    .with_state(TestState),
            };
            let handle = tokio::spawn(async move {
                axum::serve(listener, router).await.expect("serve router");
            });

            RemoteToolFailureServer {
                base_url: format!("http://{}", addr),
                handle,
            }
        })
    })
}

#[derive(Clone, Copy)]
enum RemoteToolFailureRoute {
    Catalog,
    ExecuteBadRequest,
    ExecuteForbidden,
    ExecuteRateLimited,
    ExecuteBadGateway,
    ExecuteInternalError,
}

async fn reject_execute_bad_gateway(
    State(_state): State<TestState>,
    Path(_job_id): Path<Uuid>,
    Json(_req): Json<RemoteToolExecutionRequest>,
) -> (StatusCode, &'static str) {
    (StatusCode::BAD_GATEWAY, "proxy failure")
}

async fn reject_execute_internal_error(
    State(_state): State<TestState>,
    Path(_job_id): Path<Uuid>,
    Json(_req): Json<RemoteToolExecutionRequest>,
) -> (StatusCode, &'static str) {
    (StatusCode::INTERNAL_SERVER_ERROR, "remote tool blew up")
}

type RemoteToolFailureServerFactory =
    Box<dyn Fn(RemoteToolFailureRoute) -> RemoteToolFailureServerFuture + Send + Sync>;

async fn reject_execute_rate_limited(
    State(_state): State<TestState>,
    Path(_job_id): Path<Uuid>,
    Json(_req): Json<RemoteToolExecutionRequest>,
) -> axum::response::Response {
    (
        StatusCode::TOO_MANY_REQUESTS,
        [("retry-after", "7")],
        "slow down",
    )
        .into_response()
}

// Transport type serialization fidelity tests

#[fixture]
fn sample_catalog_response() -> RemoteToolCatalogResponse {
    RemoteToolCatalogResponse {
        tools: vec![crate::llm::ToolDefinition {
            name: "test_tool".to_string(),
            description: "A **complex** test tool with UTF-8: \u{1F680}\u{1F4A1}.".to_string(),
            parameters: json!({
                "type": "object",
                "title": "TestParams",
                "properties": {
                    "query": {
                        "type": "string",
                        "minLength": 1,
                        "maxLength": 100
                    },
                    "options": {
                        "type": "object",
                        "properties": {
                            "limit": {"type": "integer", "minimum": 1, "maximum": 50}
                        },
                        "required": ["limit"]
                    }
                },
                "required": ["query", "options"]
            }),
        }],
        toolset_instructions: vec![
            "Prefer remote tools for external systems.".to_string(),
            "Use local tools for filesystem operations.".to_string(),
        ],
        catalog_version: 42,
    }
}

#[fixture]
fn sample_execution_request() -> RemoteToolExecutionRequest {
    RemoteToolExecutionRequest {
        tool_name: "complex_tool".to_string(),
        params: json!({
            "query": "test query",
            "options": {"limit": 25}
        }),
    }
}

#[fixture]
fn sample_execution_response() -> RemoteToolExecutionResponse {
    RemoteToolExecutionResponse {
        output: crate::tools::ToolOutput::success(
            json!({"result": "success", "data": [1, 2, 3]}),
            std::time::Duration::from_millis(42),
        )
        .with_cost(rust_decimal::Decimal::new(150, 2))
        .with_raw("raw execution output"),
    }
}

#[test]
fn worker_and_orchestrator_share_remote_tool_route_constants() {
    assert_eq!(
        REMOTE_TOOL_CATALOG_ROUTE,
        "/worker/{job_id}/tools/catalog",
        "catalog route constant must match the expected orchestrator route"
    );
    assert_eq!(
        REMOTE_TOOL_EXECUTE_ROUTE,
        "/worker/{job_id}/tools/execute",
        "execute route constant must match the expected orchestrator route"
    );

    let test_job_id = "12345678-1234-1234-1234-123456789012";
    let catalog_route = REMOTE_TOOL_CATALOG_ROUTE.replace("{job_id}", test_job_id);
    let execute_route = REMOTE_TOOL_EXECUTE_ROUTE.replace("{job_id}", test_job_id);

    assert_eq!(
        catalog_route,
        format!("/worker/{}/tools/catalog", test_job_id),
        "catalog route must expand job_id parameter correctly"
    );
    assert_eq!(
        execute_route,
        format!("/worker/{}/tools/execute", test_job_id),
        "execute route must expand job_id parameter correctly"
    );
}

#[rstest]
fn remote_tool_catalog_response_round_trip_without_field_loss(
    sample_catalog_response: RemoteToolCatalogResponse,
) {
    let serialized =
        serde_json::to_string(&sample_catalog_response).expect("serialize RemoteToolCatalogResponse");
    let deserialized: RemoteToolCatalogResponse =
        serde_json::from_str(&serialized).expect("deserialize RemoteToolCatalogResponse");

    assert_eq!(
        deserialized, sample_catalog_response,
        "catalog response must round-trip without field loss"
    );
}

#[rstest]
fn remote_tool_execution_request_round_trip_without_field_loss(
    sample_execution_request: RemoteToolExecutionRequest,
) {
    let serialized = serde_json::to_string(&sample_execution_request)
        .expect("serialize RemoteToolExecutionRequest");
    let deserialized: RemoteToolExecutionRequest =
        serde_json::from_str(&serialized).expect("deserialize RemoteToolExecutionRequest");

    assert_eq!(deserialized.tool_name, sample_execution_request.tool_name);
    assert_eq!(deserialized.params, sample_execution_request.params);
}

#[rstest]
fn remote_tool_execution_response_round_trip_without_field_loss(
    sample_execution_response: RemoteToolExecutionResponse,
) {
    let serialized = serde_json::to_string(&sample_execution_response)
        .expect("serialize RemoteToolExecutionResponse");
    let deserialized: RemoteToolExecutionResponse =
        serde_json::from_str(&serialized).expect("deserialize RemoteToolExecutionResponse");

    assert_eq!(
        deserialized.output.result,
        sample_execution_response.output.result
    );
    assert_eq!(
        deserialized.output.cost,
        sample_execution_response.output.cost
    );
    assert_eq!(deserialized.output.raw, sample_execution_response.output.raw);
    assert_eq!(
        deserialized.output.duration,
        sample_execution_response.output.duration
    );
}
