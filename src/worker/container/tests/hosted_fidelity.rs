//! End-to-end fidelity tests for hosted remote-tool proxy definitions.
//!
//! Verifies that a complex `ToolDefinition` round-trips through the
//! orchestrator catalogue endpoint and the worker-side proxy without
//! field loss or transformation.

use std::sync::Arc;

use anyhow::Context as _;
use axum::extract::{Path, State};
use axum::routing::{get, post};
use axum::{Json, Router};
use rstest::{fixture, rstest};
use tokio::net::TcpListener;
use tokio::sync::Mutex;
use uuid::Uuid;

use crate::llm::{Reasoning, RespondResult, ToolDefinition};
use crate::worker::api::ProxyFinishReason;
use crate::worker::api::{
    LLM_COMPLETE_WITH_TOOLS_ROUTE, ProxyToolCompletionRequest, ProxyToolCompletionResponse,
    RemoteToolCatalogResponse, WorkerHttpClient,
};
use crate::worker::container::{WorkerConfig, WorkerRuntime};

/// Test harness containing a [`WorkerRuntime`] configured with remote tools
/// and the server join handle for shutdown.
pub struct HostedCatalogHarness {
    /// The worker runtime with remote tools registered.
    pub runtime: WorkerRuntime,
    /// Join handle for the background test server.
    pub server: tokio::task::JoinHandle<()>,
    /// Captured proxied LLM tool-completion requests sent by the worker.
    pub captured_requests: Arc<Mutex<Vec<ProxyToolCompletionRequest>>>,
}

/// Builds the canonical orchestrator-owned WASM tool definition used by
/// fidelity and first-call assertion tests.
fn complex_orchestrator_wasm_tool_definition() -> ToolDefinition {
    crate::test_support::build_complex_tool_definition(
        "complex_orchestrator_wasm_fidelity_fixture",
        concat!(
            "A **complex** orchestrator-owned WASM tool for end-to-end fidelity ",
            "testing. ",
            "Handles UTF-8: \u{1F680}\u{1F4A1}. ",
            "Supports `inline code` and [markdown](https://example.com). ",
            "Special chars: <>&\"'{}[]()."
        ),
    )
}

/// Axum handler that returns a catalog containing [`complex_orchestrator_wasm_tool_definition`]
/// for the remote-tool catalog route.
async fn remote_tool_catalog_with_complex_tool(
    State(_): State<HostedCatalogTestState>,
    Path(_job_id): Path<Uuid>,
) -> Json<RemoteToolCatalogResponse> {
    Json(RemoteToolCatalogResponse {
        tools: vec![complex_orchestrator_wasm_tool_definition()],
        toolset_instructions: vec![],
        catalog_version: 99,
    })
}

/// Shared state for the hosted-catalog test server, holding the buffer of
/// captured proxied LLM tool-completion requests.
#[derive(Clone, Default)]
struct HostedCatalogTestState {
    captured_requests: Arc<Mutex<Vec<ProxyToolCompletionRequest>>>,
}

/// Axum handler that records each incoming [`ProxyToolCompletionRequest`]
/// and returns a deterministic stub completion response.
async fn capture_llm_complete_with_tools(
    State(state): State<HostedCatalogTestState>,
    Path(_job_id): Path<Uuid>,
    Json(request): Json<ProxyToolCompletionRequest>,
) -> Json<ProxyToolCompletionResponse> {
    state.captured_requests.lock().await.push(request);

    Json(ProxyToolCompletionResponse {
        content: Some("Hosted request captured.".to_string()),
        tool_calls: Vec::new(),
        input_tokens: 1,
        output_tokens: 1,
        finish_reason: ProxyFinishReason::Stop,
        cache_read_input_tokens: 0,
        cache_creation_input_tokens: 0,
    })
}

/// Spawns a local Axum server that serves the complex tool catalog and
/// captures proxied LLM tool-completion requests.
///
/// Returns the base URL, a handle to the captured-requests buffer, and the
/// server join handle.
async fn spawn_hosted_catalog_server() -> Result<
    (
        String,
        Arc<Mutex<Vec<ProxyToolCompletionRequest>>>,
        tokio::task::JoinHandle<()>,
    ),
    anyhow::Error,
> {
    let state = HostedCatalogTestState::default();
    let captured_requests = Arc::clone(&state.captured_requests);
    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let addr = listener.local_addr()?;
    let router = Router::new()
        .route(
            crate::worker::api::REMOTE_TOOL_CATALOG_ROUTE,
            get(remote_tool_catalog_with_complex_tool),
        )
        .route(
            LLM_COMPLETE_WITH_TOOLS_ROUTE,
            post(capture_llm_complete_with_tools),
        )
        .with_state(state);
    let server = tokio::spawn(async move {
        axum::serve(listener, router)
            .await
            .expect("serve hosted fidelity test router")
    });

    Ok((format!("http://{addr}"), captured_requests, server))
}

/// rstest fixture that starts a [`spawn_hosted_catalog_server`] instance and
/// builds a [`HostedCatalogHarness`] wired to it.
#[fixture]
async fn hosted_catalog_harness() -> Result<HostedCatalogHarness, Box<dyn std::error::Error>> {
    let (base_url, captured_requests, server) = spawn_hosted_catalog_server().await?;

    let client = Arc::new(
        WorkerHttpClient::new(base_url.clone(), Uuid::nil(), "test".to_string())
            .context("test client should build")?,
    );
    let runtime = WorkerRuntime::new(
        WorkerConfig {
            job_id: Uuid::nil(),
            orchestrator_url: base_url,
            ..WorkerConfig::default()
        },
        client,
    )
    .context("test runtime should build")?;

    Ok(HostedCatalogHarness {
        runtime,
        server,
        captured_requests,
    })
}

#[rstest]
#[tokio::test]
async fn hosted_worker_proxy_definition_matches_orchestrator_canonical_definition(
    #[future] hosted_catalog_harness: Result<HostedCatalogHarness, Box<dyn std::error::Error>>,
) -> Result<(), Box<dyn std::error::Error>> {
    let HostedCatalogHarness {
        runtime, server, ..
    } = hosted_catalog_harness.await?;

    runtime.register_remote_tools().await?;

    let proxy_tool = runtime
        .tools
        .get("complex_orchestrator_wasm_fidelity_fixture")
        .await
        .expect("complex tool proxy should be registered");

    let proxy_definition = ToolDefinition {
        name: proxy_tool.name().to_string(),
        description: proxy_tool.description().to_string(),
        parameters: proxy_tool.parameters_schema(),
    };

    let expected = complex_orchestrator_wasm_tool_definition();

    assert_eq!(
        proxy_definition, expected,
        "worker-advertised proxy definition must match orchestrator canonical definition exactly"
    );

    server.abort();
    let _ = server.await;
    Ok(())
}

#[rstest]
#[tokio::test]
async fn hosted_worker_first_llm_request_forwards_wasm_schema_on_first_call(
    #[future] hosted_catalog_harness: Result<HostedCatalogHarness, Box<dyn std::error::Error>>,
) -> Result<(), Box<dyn std::error::Error>> {
    let HostedCatalogHarness {
        runtime,
        server,
        captured_requests,
    } = hosted_catalog_harness.await?;

    runtime.register_remote_tools().await?;

    let reason_ctx = runtime
        .build_reasoning_context(&crate::worker::api::JobDescription {
            title: "Capture first hosted request".to_string(),
            description: "Inspect the first proxied tool-capable LLM request.".to_string(),
            project_dir: None,
        })
        .await;
    let reasoning = Reasoning::new(Arc::clone(&runtime.llm));
    let output = reasoning.respond_with_tools(&reason_ctx).await?;

    match output.result {
        RespondResult::Text(text) => {
            assert!(
                text.contains("Hosted request captured"),
                "expected the proxied hosted stub response, got: {text}"
            );
        }
        other => panic!("expected a text response from the hosted capture stub, got {other:?}"),
    }

    let captured_requests = captured_requests.lock().await;
    let first_request = captured_requests
        .first()
        .expect("expected one proxied tool-completion request");
    let forwarded_wasm_tool = first_request
        .tools
        .iter()
        .find(|tool| tool.name == "complex_orchestrator_wasm_fidelity_fixture")
        .expect("worker should forward the orchestrator-owned WASM tool on the first request");

    assert_eq!(
        forwarded_wasm_tool,
        &complex_orchestrator_wasm_tool_definition(),
        "the first proxied request must preserve the orchestrator-owned WASM schema exactly"
    );

    drop(captured_requests);
    server.abort();
    let _ = server.await;
    Ok(())
}
