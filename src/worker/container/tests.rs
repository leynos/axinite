//! Unit tests for the container worker runtime and its tool-advertising paths.

use std::sync::Arc;

use axum::extract::{Path, State};
use axum::routing::get;
use axum::{Json, Router};
use rstest::rstest;
use uuid::Uuid;

use super::*;
use crate::llm::ToolDefinition;
use crate::worker::api::{REMOTE_TOOL_CATALOG_ROUTE, RemoteToolCatalogResponse};

#[rstest]
#[tokio::test]
async fn worker_runtime_build_tools_preserves_container_local_tools() {
    let mut names = WorkerRuntime::build_tools().list().await;
    names.sort();

    assert_eq!(
        names,
        vec![
            "apply_patch",
            "list_dir",
            "read_file",
            "shell",
            "write_file"
        ]
    );
}

#[derive(Clone)]
struct TestState;

async fn remote_tool_catalog(
    State(_state): State<TestState>,
    Path(_job_id): Path<Uuid>,
) -> Json<RemoteToolCatalogResponse> {
    Json(RemoteToolCatalogResponse {
        tools: vec![ToolDefinition {
            name: "hosted_worker_remote_tool_fixture".to_string(),
            description: "Remote tool from orchestrator catalog".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "query": {"type": "string"}
                },
                "required": ["query"]
            }),
        }],
        toolset_instructions: vec![],
        catalog_version: 42,
    })
}

#[rstest]
#[tokio::test]
async fn hosted_worker_remote_tool_catalog_registers_remote_tools() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind listener");
    let addr = listener.local_addr().expect("listener addr");
    let router = Router::new()
        .route(REMOTE_TOOL_CATALOG_ROUTE, get(remote_tool_catalog))
        .with_state(TestState);
    let server = tokio::spawn(async move {
        axum::serve(listener, router).await.expect("serve router");
    });

    let client = Arc::new(WorkerHttpClient::new(
        format!("http://{}", addr),
        Uuid::nil(),
        "test".to_string(),
    ));
    let runtime = WorkerRuntime::from_client(
        WorkerConfig {
            job_id: Uuid::nil(),
            orchestrator_url: format!("http://{}", addr),
            ..WorkerConfig::default()
        },
        client,
    );

    runtime
        .register_remote_tools()
        .await
        .expect("register hosted remote tools");

    let names: Vec<String> = runtime
        .tools
        .tool_definitions()
        .await
        .into_iter()
        .map(|def| def.name)
        .collect();

    assert_eq!(
        names,
        vec![
            "apply_patch",
            "hosted_worker_remote_tool_fixture",
            "list_dir",
            "read_file",
            "shell",
            "write_file",
        ]
    );

    let remote_tool = runtime
        .tools
        .get("hosted_worker_remote_tool_fixture")
        .await
        .expect("hosted remote tool should be registered");
    assert_eq!(
        remote_tool.description(),
        "Remote tool from orchestrator catalog"
    );
    assert_eq!(remote_tool.parameters_schema()["required"][0], "query");

    server.abort();
    let _ = server.await;
}
