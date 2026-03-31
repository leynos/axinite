//! Tests for remote tool catalog registration and toolset instruction merging.

use std::sync::Arc;

use axum::extract::{Path, State};
use axum::routing::get;
use axum::{Json, Router};
use rstest::rstest;
use tokio::net::TcpListener;
use tokio::sync::Mutex;
use uuid::Uuid;

use crate::agent::agentic_loop::LoopDelegate;
use crate::llm::ToolDefinition;
use crate::worker::api::{JobDescription, REMOTE_TOOL_CATALOG_ROUTE, RemoteToolCatalogResponse};
use crate::worker::container::delegate::ContainerDelegate;
use crate::worker::container::{
    HOSTED_GUIDANCE_HEADING, WorkerConfig, WorkerHttpClient, WorkerRuntime,
};

fn expected_remote_tool_definition() -> ToolDefinition {
    ToolDefinition {
        name: "hosted_worker_remote_tool_fixture".to_string(),
        description: "Remote tool from orchestrator catalog".to_string(),
        parameters: serde_json::json!({
            "type": "object",
            "properties": {
                "query": {"type": "string"}
            },
            "required": ["query"]
        }),
    }
}

fn expected_merged_tool_names() -> Vec<String> {
    let mut names = expected_local_tool_names();
    names.push(expected_remote_tool_definition().name);
    names.sort();
    names
}

fn expected_local_tool_names() -> Vec<String> {
    vec![
        "apply_patch".to_string(),
        "list_dir".to_string(),
        "read_file".to_string(),
        "shell".to_string(),
        "write_file".to_string(),
    ]
}

async fn remote_tool_catalog(
    State(_state): State<TestState>,
    Path(_job_id): Path<Uuid>,
) -> Json<RemoteToolCatalogResponse> {
    Json(RemoteToolCatalogResponse {
        tools: vec![expected_remote_tool_definition()],
        toolset_instructions: vec!["Prefer hosted remote tools for external systems.".to_string()],
        catalog_version: 42,
    })
}

async fn remote_tool_catalog_error(
    State(_state): State<TestState>,
    Path(_job_id): Path<Uuid>,
) -> (axum::http::StatusCode, &'static str) {
    (
        axum::http::StatusCode::INTERNAL_SERVER_ERROR,
        "catalog offline",
    )
}

/// Minimal shared Axum state for test catalog routers.
#[derive(Clone)]
pub(super) struct TestState;

/// Spawns an ephemeral Axum server that serves the remote-tool catalog route.
///
/// `H` is any Axum handler compatible with [`TestState`], and `T` is its
/// extractor tuple. Returns the `http://host:port` base URL and a join
/// handle for the background server task.
pub(super) async fn spawn_test_server<H, T>(
    handler: H,
) -> Result<(String, tokio::task::JoinHandle<()>), Box<dyn std::error::Error>>
where
    H: axum::handler::Handler<T, TestState> + Clone + Send + 'static,
    T: 'static,
{
    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let addr = listener.local_addr()?;
    let router = Router::new()
        .route(REMOTE_TOOL_CATALOG_ROUTE, get(handler))
        .with_state(TestState);
    let server = tokio::spawn(async move {
        axum::serve(listener, router)
            .await
            .expect("serve router in test server")
    });
    Ok((format!("http://{addr}"), server))
}

async fn spawn_hosted_guidance_catalog_server()
-> Result<(String, tokio::task::JoinHandle<()>), Box<dyn std::error::Error>> {
    spawn_test_server(remote_tool_catalog).await
}

async fn build_runtime_with_remote_tools(
    base_url: &str,
) -> Result<(WorkerRuntime, Arc<WorkerHttpClient>), anyhow::Error> {
    let client = Arc::new(WorkerHttpClient::new(
        base_url.to_string(),
        Uuid::nil(),
        "test".to_string(),
    )?);
    let mut runtime = WorkerRuntime::new(
        WorkerConfig {
            job_id: Uuid::nil(),
            orchestrator_url: base_url.to_string(),
            ..WorkerConfig::default()
        },
        Arc::clone(&client),
    )?;
    runtime.toolset_instructions = runtime.register_remote_tools().await?;
    Ok((runtime, client))
}

#[rstest]
#[tokio::test]
async fn hosted_worker_remote_tool_catalog_registers_remote_tools()
-> Result<(), Box<dyn std::error::Error>> {
    let (base_url, server) = spawn_hosted_guidance_catalog_server().await?;

    let client = Arc::new(
        WorkerHttpClient::new(base_url.clone(), Uuid::nil(), "test".to_string())
            .expect("test client should build"),
    );
    let runtime = WorkerRuntime::new(
        WorkerConfig {
            job_id: Uuid::nil(),
            orchestrator_url: base_url,
            ..WorkerConfig::default()
        },
        client,
    )?;

    runtime.register_remote_tools().await?;

    let definitions: Vec<crate::llm::ToolDefinition> = runtime.tools.tool_definitions().await;
    let mut names: Vec<String> = definitions.into_iter().map(|def| def.name).collect();
    names.sort();

    assert_eq!(names, expected_merged_tool_names());

    let remote_tool: Option<std::sync::Arc<dyn crate::tools::Tool>> =
        runtime.tools.get("hosted_worker_remote_tool_fixture").await;
    let remote_tool = remote_tool.expect("hosted remote tool should be registered");
    let expected = expected_remote_tool_definition();
    assert_eq!(remote_tool.name(), expected.name);
    assert_eq!(remote_tool.description(), expected.description);
    assert_eq!(remote_tool.parameters_schema(), expected.parameters);

    server.abort();
    let _ = server.await;
    Ok(())
}

#[rstest]
#[tokio::test]
async fn worker_runtime_build_reasoning_context_merges_local_and_remote_tools()
-> Result<(), Box<dyn std::error::Error>> {
    let (base_url, server) = spawn_hosted_guidance_catalog_server().await?;
    let (runtime, _client) = build_runtime_with_remote_tools(&base_url).await?;

    let reason_ctx = runtime
        .build_reasoning_context(&JobDescription {
            title: "Hosted guidance".to_string(),
            description: "Use the available tools".to_string(),
            project_dir: None,
        })
        .await;

    let guidance_message = reason_ctx
        .messages
        .iter()
        .find(|message| message.content.contains(HOSTED_GUIDANCE_HEADING))
        .expect("expected hosted remote-tool guidance message");

    assert!(
        guidance_message
            .content
            .contains("Prefer hosted remote tools for external systems."),
        "reasoning context should include the preserved orchestrator guidance"
    );

    let mut actual_tool_names: Vec<_> = reason_ctx
        .available_tools
        .iter()
        .map(|tool| tool.name.clone())
        .collect();
    actual_tool_names.sort();

    assert_eq!(actual_tool_names, expected_merged_tool_names());

    let remote_tool = reason_ctx
        .available_tools
        .iter()
        .find(|tool| tool.name == "hosted_worker_remote_tool_fixture")
        .expect("reasoning context should expose the hosted remote tool");
    let expected = expected_remote_tool_definition();
    assert_eq!(remote_tool.description, expected.description);
    assert_eq!(remote_tool.parameters, expected.parameters);

    server.abort();
    let _ = server.await;
    Ok(())
}

#[rstest]
#[tokio::test]
async fn worker_runtime_refresh_keeps_merged_tools_without_duplicate_guidance()
-> Result<(), Box<dyn std::error::Error>> {
    let (base_url, server) = spawn_hosted_guidance_catalog_server().await?;
    let (runtime, client) = build_runtime_with_remote_tools(&base_url).await?;

    let mut reason_ctx = runtime
        .build_reasoning_context(&JobDescription {
            title: "Hosted guidance".to_string(),
            description: "Use the available tools".to_string(),
            project_dir: None,
        })
        .await;

    let guidance_before = reason_ctx
        .messages
        .iter()
        .filter(|message| message.content.contains(HOSTED_GUIDANCE_HEADING))
        .count();
    assert_eq!(
        guidance_before, 1,
        "expected one guidance message before refresh"
    );

    let delegate = ContainerDelegate::new(
        client,
        Arc::clone(&runtime.safety),
        Arc::clone(&runtime.tools),
        Arc::clone(&runtime.extra_env),
        Arc::new(Mutex::new(0)),
    );

    let outcome = delegate.before_llm_call(&mut reason_ctx, 1).await;
    assert!(
        outcome.is_none(),
        "before_llm_call should not terminate the loop"
    );

    let mut actual_tool_names: Vec<_> = reason_ctx
        .available_tools
        .iter()
        .map(|tool| tool.name.clone())
        .collect();
    actual_tool_names.sort();

    assert_eq!(actual_tool_names, expected_merged_tool_names());
    assert_eq!(
        reason_ctx
            .messages
            .iter()
            .filter(|message| message.content.contains(HOSTED_GUIDANCE_HEADING))
            .count(),
        1,
        "refresh should not duplicate hosted remote-tool guidance"
    );

    server.abort();
    let _ = server.await;
    Ok(())
}

#[rstest]
#[tokio::test]
async fn hosted_worker_remote_tool_catalog_degraded_startup_keeps_local_tools()
-> Result<(), Box<dyn std::error::Error>> {
    let (base_url, server) = spawn_test_server(remote_tool_catalog_error).await?;

    let client = Arc::new(
        WorkerHttpClient::new(base_url.clone(), Uuid::nil(), "test".to_string())
            .expect("test client should build"),
    );
    let runtime = WorkerRuntime::new(
        WorkerConfig {
            job_id: Uuid::nil(),
            orchestrator_url: base_url,
            ..WorkerConfig::default()
        },
        client,
    )?;

    runtime.register_remote_tools_with_degraded_startup().await;

    let reason_ctx = runtime
        .build_reasoning_context(&JobDescription {
            title: "Degraded startup".to_string(),
            description: "Continue with local tools only".to_string(),
            project_dir: None,
        })
        .await;

    let mut actual_tool_names: Vec<_> = reason_ctx
        .available_tools
        .iter()
        .map(|tool| tool.name.clone())
        .collect();
    actual_tool_names.sort();

    assert_eq!(actual_tool_names, expected_local_tool_names());
    assert!(
        reason_ctx
            .messages
            .iter()
            .all(|message| !message.content.contains(HOSTED_GUIDANCE_HEADING)),
        "degraded startup should not inject hosted guidance"
    );

    server.abort();
    let _ = server.await;
    Ok(())
}
