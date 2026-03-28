//! Tests for the worker-side remote tool proxy.

use std::sync::Arc;

use anyhow::Context;
use axum::extract::{Path, State};
use axum::routing::post;
use axum::{Json, Router};
use rstest::{fixture, rstest};
use rust_decimal::Decimal;
use tokio::sync::Mutex;
use uuid::Uuid;

use super::*;
use crate::worker::api::{
    REMOTE_TOOL_EXECUTE_ROUTE, RemoteToolExecutionRequest, RemoteToolExecutionResponse,
};

#[derive(Clone)]
struct TestState;

#[derive(Clone)]
struct RouteCapturingState {
    received_requests: Arc<Mutex<Vec<(String, Uuid, String)>>>,
}

async fn execute_tool(
    State(_state): State<TestState>,
    Path(job_id): Path<Uuid>,
    Json(req): Json<RemoteToolExecutionRequest>,
) -> Json<RemoteToolExecutionResponse> {
    Json(RemoteToolExecutionResponse {
        output: ToolOutput::success(
            serde_json::json!({
                "job_id": job_id,
                "tool_name": req.tool_name,
                "params": req.params,
            }),
            std::time::Duration::from_millis(7),
        )
        .with_cost(Decimal::new(125, 2))
        .with_raw("proxy raw output"),
    })
}

/// Bundles the in-process execute-route server and a pre-wired HTTP client.
struct ProxyTestServer {
    client: Arc<WorkerHttpClient>,
    job_id: Uuid,
    server: tokio::task::JoinHandle<()>,
}

/// Spins up a local Axum server wired to `execute_tool` and returns a
/// `WorkerHttpClient` pointed at it, together with the job id in use.
#[fixture]
async fn proxy_test_server() -> anyhow::Result<ProxyTestServer> {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .context("bind listener")?;
    let addr = listener.local_addr().context("listener addr")?;
    let router = Router::new()
        .route(REMOTE_TOOL_EXECUTE_ROUTE, post(execute_tool))
        .with_state(TestState);
    let server = tokio::spawn(async move {
        let _ = axum::serve(listener, router).await;
    });
    let job_id = Uuid::new_v4();
    let client = Arc::new(
        WorkerHttpClient::new(format!("http://{}", addr), job_id, "test-token".to_string())
            .context("test client should build")?,
    );
    Ok(ProxyTestServer {
        client,
        job_id,
        server,
    })
}

#[rstest]
#[tokio::test]
async fn remote_tool_execute_round_trips_catalog_tools(
    #[future] proxy_test_server: anyhow::Result<ProxyTestServer>,
) -> anyhow::Result<()> {
    let ProxyTestServer {
        client,
        job_id,
        server,
    } = proxy_test_server.await?;
    let registry = ToolRegistry::new();
    let expected_definition = ToolDefinition {
        name: "github_search".to_string(),
        description: "Search repositories".to_string(),
        parameters: serde_json::json!({
            "type": "object",
            "properties": {
                "query": {"type": "string"}
            },
            "required": ["query"]
        }),
    };
    register_worker_remote_tool_proxies(&registry, client, vec![expected_definition.clone()]);

    let tool = registry
        .get("github_search")
        .await
        .context("github_search proxy must be registered")?;
    let output = tool
        .execute(
            serde_json::json!({"query": "axinite"}),
            &JobContext::default(),
        )
        .await
        .context("proxy execution should succeed")?;

    assert_eq!(tool.name(), expected_definition.name);
    assert_eq!(tool.description(), expected_definition.description);
    assert_eq!(tool.parameters_schema(), expected_definition.parameters);
    assert_eq!(output.result["tool_name"], "github_search");
    assert_eq!(output.result["job_id"], job_id.to_string());
    assert_eq!(output.result["params"]["query"], "axinite");
    assert_eq!(output.cost, Some(Decimal::new(125, 2)));
    assert_eq!(output.raw.as_deref(), Some("proxy raw output"));
    assert_eq!(output.duration, std::time::Duration::from_millis(7));

    server.abort();
    let _ = server.await;
    Ok(())
}

#[rstest]
#[tokio::test]
async fn worker_remote_tool_proxy_preserves_full_tool_output_fields(
    #[future] proxy_test_server: anyhow::Result<ProxyTestServer>,
) -> anyhow::Result<()> {
    let ProxyTestServer {
        client,
        job_id,
        server,
    } = proxy_test_server.await?;
    let proxy = WorkerRemoteToolProxy::new(
        ToolDefinition {
            name: "output_fidelity_tool".to_string(),
            description: "Tests full output preservation".to_string(),
            parameters: serde_json::json!({"type": "object", "properties": {}}),
        },
        client,
    );

    let output = proxy
        .execute(serde_json::json!({"test": "data"}), &JobContext::default())
        .await
        .context("proxy execution should succeed")?;

    assert_eq!(output.result["job_id"], job_id.to_string());
    assert_eq!(output.result["tool_name"], "output_fidelity_tool");
    assert_eq!(output.result["params"]["test"], "data");
    assert_eq!(
        output.cost,
        Some(Decimal::new(125, 2)),
        "proxy must preserve cost field"
    );
    assert_eq!(
        output.raw.as_deref(),
        Some("proxy raw output"),
        "proxy must preserve raw field"
    );
    assert_eq!(
        output.duration,
        std::time::Duration::from_millis(7),
        "proxy must preserve duration field"
    );

    server.abort();
    let _ = server.await;
    Ok(())
}

#[tokio::test]
async fn worker_remote_tool_proxy_preserves_full_tool_definition_fields() {
    let complex_definition = crate::test_support::build_complex_tool_definition(
        "complex_proxy_fixture",
        "Complex tool for proxy definition fidelity testing",
    );

    let client = Arc::new(
        WorkerHttpClient::new(
            "http://127.0.0.1:0".to_string(),
            Uuid::new_v4(),
            "test-token".to_string(),
        )
        .expect("test client should build"),
    );
    let proxy = WorkerRemoteToolProxy::new(complex_definition.clone(), client);

    let reconstructed = ToolDefinition {
        name: proxy.name().to_string(),
        description: proxy.description().to_string(),
        parameters: proxy.parameters_schema(),
    };

    assert_eq!(
        reconstructed, complex_definition,
        "proxy-reported fields must reconstruct the original definition exactly"
    );
}

async fn execute_tool_with_route_capture(
    State(state): State<RouteCapturingState>,
    Path(job_id): Path<Uuid>,
    axum::extract::OriginalUri(original_uri): axum::extract::OriginalUri,
    Json(req): Json<RemoteToolExecutionRequest>,
) -> Json<RemoteToolExecutionResponse> {
    state.received_requests.lock().await.push((
        original_uri.path().to_string(),
        job_id,
        req.tool_name.clone(),
    ));
    Json(RemoteToolExecutionResponse {
        output: ToolOutput::success(
            serde_json::json!({"executed": true}),
            std::time::Duration::from_millis(5),
        ),
    })
}

#[tokio::test]
async fn worker_remote_tool_proxy_routes_execution_through_orchestrator_endpoint()
-> anyhow::Result<()> {
    let state = RouteCapturingState {
        received_requests: Arc::new(Mutex::new(Vec::new())),
    };

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .context("bind listener")?;
    let addr = listener.local_addr().context("listener addr")?;
    let router = Router::new()
        .route(
            REMOTE_TOOL_EXECUTE_ROUTE,
            post(execute_tool_with_route_capture),
        )
        .with_state(state.clone());
    let server = tokio::spawn(async move {
        let _ = axum::serve(listener, router).await;
    });

    let job_id = Uuid::new_v4();
    let client = Arc::new(
        WorkerHttpClient::new(format!("http://{}", addr), job_id, "test-token".to_string())
            .context("test client should build")?,
    );
    let proxy = WorkerRemoteToolProxy::new(
        ToolDefinition {
            name: "route_test_tool".to_string(),
            description: "Tests route path".to_string(),
            parameters: serde_json::json!({"type": "object", "properties": {}}),
        },
        client,
    );

    proxy
        .execute(serde_json::json!({}), &JobContext::default())
        .await
        .context("proxy execution should succeed")?;

    let requests = state.received_requests.lock().await;
    assert_eq!(
        requests.len(),
        1,
        "proxy must send exactly one request to the orchestrator"
    );

    let (route_path, received_job_id, tool_name) = &requests[0];
    assert_eq!(
        route_path,
        &format!("/worker/{}/tools/execute", job_id),
        "proxy must route execution through the correct orchestrator endpoint"
    );
    assert_eq!(received_job_id, &job_id);
    assert_eq!(tool_name, "route_test_tool");

    server.abort();
    let _ = server.await;
    Ok(())
}
