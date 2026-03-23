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

#[derive(Clone)]
struct TestState;

async fn spawn_test_server<H, T>(
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
) -> Result<(WorkerRuntime, Arc<WorkerHttpClient>), Box<dyn std::error::Error>> {
    let client = Arc::new(WorkerHttpClient::new(
        base_url.to_string(),
        Uuid::nil(),
        "test".to_string(),
    ));
    let mut runtime = WorkerRuntime::from_client(
        WorkerConfig {
            job_id: Uuid::nil(),
            orchestrator_url: base_url.to_string(),
            ..WorkerConfig::default()
        },
        Arc::clone(&client),
    );
    runtime.toolset_instructions = runtime.register_remote_tools().await?;
    Ok((runtime, client))
}

#[rstest]
#[tokio::test]
async fn hosted_worker_remote_tool_catalog_registers_remote_tools()
-> Result<(), Box<dyn std::error::Error>> {
    let (base_url, server) = spawn_hosted_guidance_catalog_server().await?;

    let client = Arc::new(WorkerHttpClient::new(
        base_url.clone(),
        Uuid::nil(),
        "test".to_string(),
    ));
    let runtime = WorkerRuntime::from_client(
        WorkerConfig {
            job_id: Uuid::nil(),
            orchestrator_url: base_url,
            ..WorkerConfig::default()
        },
        client,
    );

    runtime.register_remote_tools().await?;

    let mut names: Vec<String> = runtime
        .tools
        .tool_definitions()
        .await
        .into_iter()
        .map(|def| def.name)
        .collect();
    names.sort();

    assert_eq!(names, expected_merged_tool_names());

    let remote_tool = runtime
        .tools
        .get("hosted_worker_remote_tool_fixture")
        .await
        .expect("hosted remote tool should be registered");
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

    let delegate = ContainerDelegate {
        client,
        safety: Arc::clone(&runtime.safety),
        tools: Arc::clone(&runtime.tools),
        extra_env: Arc::clone(&runtime.extra_env),
        last_output: Mutex::new(String::new()),
        iteration_tracker: Arc::new(Mutex::new(0)),
    };

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

    let client = Arc::new(WorkerHttpClient::new(
        base_url.clone(),
        Uuid::nil(),
        "test".to_string(),
    ));
    let runtime = WorkerRuntime::from_client(
        WorkerConfig {
            job_id: Uuid::nil(),
            orchestrator_url: base_url,
            ..WorkerConfig::default()
        },
        client,
    );

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

fn complex_orchestrator_tool_definition() -> ToolDefinition {
    ToolDefinition {
        name: "complex_fidelity_fixture".to_string(),
        description: concat!(
            "A **complex** tool for end-to-end fidelity testing. ",
            "Handles UTF-8: \u{1F680}\u{1F4A1}. ",
            "Supports `inline code` and [markdown](https://example.com). ",
            "Special chars: <>&\"'{}[]()."
        )
        .to_string(),
        parameters: serde_json::json!({
            "type": "object",
            "title": "ComplexParams",
            "description": "Nested schema with multiple property types",
            "properties": {
                "query": {
                    "type": "string",
                    "description": "Search query with constraints",
                    "minLength": 1,
                    "maxLength": 500
                },
                "options": {
                    "type": "object",
                    "description": "Nested configuration object",
                    "properties": {
                        "limit": {
                            "type": "integer",
                            "minimum": 1,
                            "maximum": 100,
                            "default": 10
                        },
                        "include_metadata": {
                            "type": "boolean",
                            "default": false
                        },
                        "filters": {
                            "type": "array",
                            "items": {
                                "type": "string",
                                "enum": ["active", "archived", "draft"]
                            }
                        }
                    },
                    "required": ["limit"]
                },
                "callback_url": {
                    "type": "string",
                    "format": "uri",
                    "description": "Optional webhook URL"
                }
            },
            "required": ["query", "options"],
            "additionalProperties": false
        }),
    }
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

#[rstest]
#[tokio::test]
async fn hosted_worker_proxy_definition_matches_orchestrator_canonical_definition()
-> Result<(), Box<dyn std::error::Error>> {
    let (base_url, server) = spawn_test_server(remote_tool_catalog_with_complex_tool).await?;

    let client = Arc::new(WorkerHttpClient::new(
        base_url.clone(),
        Uuid::nil(),
        "test".to_string(),
    ));
    let runtime = WorkerRuntime::from_client(
        WorkerConfig {
            job_id: Uuid::nil(),
            orchestrator_url: base_url,
            ..WorkerConfig::default()
        },
        client,
    );

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
