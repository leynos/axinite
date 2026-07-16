//! Shared test fixtures for worker API tests.

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::routing::{get, post};
use axum::{Json, Router};
use rstest::fixture;
use serde_json::json;
use std::future::Future;
use std::pin::Pin;
use uuid::Uuid;

use crate::worker::api::{
    REMOTE_TOOL_CATALOG_ROUTE, REMOTE_TOOL_EXECUTE_ROUTE, RemoteToolCatalogResponse,
    RemoteToolExecutionRequest,
};

/// Unit type used as shared Axum state for failure-mode test routers.
///
/// Implements `Clone` and `Copy` so it can be cheaply shared across
/// handler extractors without allocation.
#[derive(Clone, Copy)]
pub struct TestState;

/// Selects which remote-tool API route the failure server should reject.
///
/// Each variant maps to a distinct HTTP status code returned by the mock
/// server, letting tests exercise individual error-handling paths in the
/// remote-tool client.
#[derive(Clone, Copy)]
pub enum RemoteToolFailureRoute {
    /// Returns `403 Forbidden` on the catalog endpoint.
    Catalog,
    /// Returns `400 Bad Request` on the execute endpoint.
    ExecuteBadRequest,
    /// Returns `403 Forbidden` on the execute endpoint.
    ExecuteForbidden,
    /// Returns `429 Too Many Requests` (with `Retry-After`) on execute.
    ExecuteRateLimited,
    /// Returns `502 Bad Gateway` on the execute endpoint.
    ExecuteBadGateway,
    /// Returns `500 Internal Server Error` on the execute endpoint.
    ExecuteInternalError,
}

/// A pinned, boxed future that resolves to a [`RemoteToolFailureServer`] or
/// an error. Returned by the factory closure so that server creation can be
/// performed asynchronously.
pub type RemoteToolFailureServerFuture =
    Pin<Box<dyn Future<Output = Result<RemoteToolFailureServer, anyhow::Error>> + Send>>;

/// A running mock server that rejects requests on a chosen route.
pub struct RemoteToolFailureServer {
    /// The `http://host:port` base URL of the listening server.
    pub base_url: String,
    /// Join handle for the background Tokio task serving requests.
    pub handle: tokio::task::JoinHandle<()>,
}

impl Drop for RemoteToolFailureServer {
    fn drop(&mut self) {
        self.handle.abort();
    }
}

/// Thread-safe factory that spawns a [`RemoteToolFailureServer`] configured
/// to fail on the given [`RemoteToolFailureRoute`].
pub type RemoteToolFailureServerFactory =
    Box<dyn Fn(RemoteToolFailureRoute) -> RemoteToolFailureServerFuture + Send + Sync>;

/// Returns a factory closure that spawns a failure-mode mock server.
///
/// Call the returned closure with a [`RemoteToolFailureRoute`] to start a
/// server bound to an ephemeral port that rejects requests on the chosen
/// route with an appropriate HTTP error status.
#[fixture]
pub fn remote_tool_failure_server() -> RemoteToolFailureServerFactory {
    Box::new(|route| {
        Box::pin(async move {
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
            let addr = listener.local_addr()?;
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
                let _ = axum::serve(listener, router).await;
            });

            Ok(RemoteToolFailureServer {
                base_url: format!("http://{}", addr),
                handle,
            })
        })
    })
}

#[fixture]
pub fn sample_catalog_response() -> RemoteToolCatalogResponse {
    RemoteToolCatalogResponse {
        tools: vec![crate::test_support::build_complex_tool_definition(
            "test_tool",
            "A **complex** test tool with UTF-8: \u{1F680}\u{1F4A1}.",
        )],
        toolset_instructions: vec![
            "Prefer remote tools for external systems.".to_string(),
            "Use local tools for filesystem operations.".to_string(),
        ],
        catalog_version: 42,
    }
}

#[fixture]
pub fn sample_execution_request() -> RemoteToolExecutionRequest {
    RemoteToolExecutionRequest {
        tool_name: "complex_tool".to_string(),
        params: json!({
            "query": "test query",
            "options": {"limit": 25}
        }),
    }
}

#[fixture]
pub fn sample_execution_response() -> crate::worker::api::RemoteToolExecutionResponse {
    crate::worker::api::RemoteToolExecutionResponse {
        output: crate::tools::ToolOutput::success(
            json!({"result": "success", "data": [1, 2, 3]}),
            std::time::Duration::from_millis(42),
        )
        .with_cost(rust_decimal::Decimal::new(150, 2))
        .with_raw("raw execution output"),
    }
}

async fn reject_catalog(
    State(_state): State<TestState>,
    Path(_job_id): Path<Uuid>,
) -> (StatusCode, &'static str) {
    (StatusCode::FORBIDDEN, "nope")
}

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
