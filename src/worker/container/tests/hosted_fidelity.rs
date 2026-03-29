//! End-to-end fidelity tests for hosted remote-tool proxy definitions.
//!
//! Verifies that a complex `ToolDefinition` round-trips through the
//! orchestrator catalogue endpoint and the worker-side proxy without
//! field loss or transformation.

use std::sync::Arc;

use axum::Json;
use axum::extract::{Path, State};
use rstest::{fixture, rstest};
use uuid::Uuid;

use crate::llm::ToolDefinition;
use crate::worker::api::{RemoteToolCatalogResponse, WorkerHttpClient};
use crate::worker::container::{WorkerConfig, WorkerRuntime};

use super::remote_tools::{TestState, spawn_test_server};

/// Test harness containing a [`WorkerRuntime`] configured with remote tools
/// and the server join handle for shutdown.
pub struct HostedCatalogHarness {
    /// The worker runtime with remote tools registered.
    pub runtime: WorkerRuntime,
    /// Join handle for the background test server.
    pub server: tokio::task::JoinHandle<()>,
}

fn complex_orchestrator_tool_definition() -> ToolDefinition {
    crate::test_support::build_complex_tool_definition(
        "complex_fidelity_fixture",
        concat!(
            "A **complex** tool for end-to-end fidelity testing. ",
            "Handles UTF-8: \u{1F680}\u{1F4A1}. ",
            "Supports `inline code` and [markdown](https://example.com). ",
            "Special chars: <>&\"'{}[]()."
        ),
    )
}

async fn remote_tool_catalog_with_complex_tool(
    State(_state): State<TestState>,
    Path(_job_id): Path<Uuid>,
) -> Json<RemoteToolCatalogResponse> {
    Json(RemoteToolCatalogResponse {
        tools: vec![complex_orchestrator_tool_definition()],
        toolset_instructions: vec![],
        catalog_version: 99,
    })
}

/// Creates a [`HostedCatalogHarness`] with a test server serving the complex
/// tool catalog.
#[fixture]
async fn hosted_catalog_harness() -> Result<HostedCatalogHarness, Box<dyn std::error::Error>> {
    let (base_url, server) = spawn_test_server(remote_tool_catalog_with_complex_tool).await?;

    let client = Arc::new(WorkerHttpClient::new(
        base_url.clone(),
        Uuid::nil(),
        "test".to_string(),
    ));
    let runtime = WorkerRuntime::new(
        WorkerConfig {
            job_id: Uuid::nil(),
            orchestrator_url: base_url,
            ..WorkerConfig::default()
        },
        client,
    );

    Ok(HostedCatalogHarness { runtime, server })
}

#[rstest]
#[tokio::test]
async fn hosted_worker_proxy_definition_matches_orchestrator_canonical_definition(
    #[future] hosted_catalog_harness: Result<HostedCatalogHarness, Box<dyn std::error::Error>>,
) -> Result<(), Box<dyn std::error::Error>> {
    let HostedCatalogHarness { runtime, server } = hosted_catalog_harness.await?;

    runtime.register_remote_tools().await?;

    let proxy_tool = runtime
        .tools
        .get("complex_fidelity_fixture")
        .await
        .expect("complex tool proxy should be registered");

    let proxy_definition = ToolDefinition {
        name: proxy_tool.name().to_string(),
        description: proxy_tool.description().to_string(),
        parameters: proxy_tool.parameters_schema(),
    };

    let expected = complex_orchestrator_tool_definition();

    assert_eq!(
        proxy_definition, expected,
        "worker-advertised proxy definition must match orchestrator canonical definition exactly"
    );

    server.abort();
    let _ = server.await;
    Ok(())
}
