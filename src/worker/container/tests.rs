//! Unit tests for the container worker runtime and its tool-advertising paths.

use std::collections::VecDeque;
use std::sync::Arc;

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::routing::{get, post};
use axum::{Json, Router};
use rstest::rstest;
use serde_json::json;
use tokio::net::TcpListener;
use tokio::sync::Mutex;
use tokio::sync::oneshot;
use uuid::Uuid;

use super::delegate::ContainerDelegate;
use super::*;
use crate::agent::agentic_loop::LoopDelegate;
use crate::error::{Error, ToolError};
use crate::llm::ToolDefinition;
use crate::worker::api::{
    CompletionReport, CredentialResponse, JobDescription, REMOTE_TOOL_CATALOG_ROUTE,
    RemoteToolCatalogResponse, StatusUpdate, WorkerState,
};

#[derive(Clone, Copy, Debug)]
enum PreLoopFailureCase {
    GetJob,
    HydrateCredentials,
}

#[derive(Default)]
struct RuntimeTestState {
    job_statuses: Mutex<VecDeque<StatusCode>>,
    credential_statuses: Mutex<VecDeque<StatusCode>>,
    status_statuses: Mutex<VecDeque<StatusCode>>,
    statuses: Mutex<Vec<StatusUpdate>>,
    completions: Mutex<Vec<CompletionReport>>,
    result_events: Mutex<Vec<serde_json::Value>>,
}

async fn take_next_status(queue: &Mutex<VecDeque<StatusCode>>, default: StatusCode) -> StatusCode {
    queue.lock().await.pop_front().unwrap_or(default)
}

async fn job_handler(State(state): State<Arc<RuntimeTestState>>) -> impl IntoResponse {
    match take_next_status(&state.job_statuses, StatusCode::OK).await {
        StatusCode::OK => (
            StatusCode::OK,
            Json(JobDescription {
                title: "Test job".to_string(),
                description: "Run a test".to_string(),
                project_dir: None,
            }),
        )
            .into_response(),
        status => status.into_response(),
    }
}

async fn credentials_handler(State(state): State<Arc<RuntimeTestState>>) -> impl IntoResponse {
    match take_next_status(&state.credential_statuses, StatusCode::OK).await {
        StatusCode::OK => (StatusCode::OK, Json(Vec::<CredentialResponse>::new())).into_response(),
        status => status.into_response(),
    }
}

async fn status_handler(
    State(state): State<Arc<RuntimeTestState>>,
    Json(update): Json<StatusUpdate>,
) -> impl IntoResponse {
    state.statuses.lock().await.push(update);
    take_next_status(&state.status_statuses, StatusCode::OK).await
}

async fn complete_handler(
    State(state): State<Arc<RuntimeTestState>>,
    Json(report): Json<CompletionReport>,
) -> impl IntoResponse {
    state.completions.lock().await.push(report);
    Json(json!({ "status": "ok" }))
}

async fn event_handler(
    State(state): State<Arc<RuntimeTestState>>,
    Json(payload): Json<crate::worker::api::JobEventPayload>,
) -> impl IntoResponse {
    if payload.event_type == crate::worker::api::JobEventType::Result {
        state.result_events.lock().await.push(payload.data);
    }
    StatusCode::OK
}

async fn prompt_handler() -> impl IntoResponse {
    StatusCode::NO_CONTENT
}

async fn spawn_runtime_test_server(
    state: Arc<RuntimeTestState>,
) -> std::io::Result<(
    String,
    oneshot::Sender<()>,
    tokio::task::JoinHandle<std::io::Result<()>>,
)> {
    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let addr = listener.local_addr()?;

    let app = Router::new()
        .route("/worker/{job_id}/job", get(job_handler))
        .route("/worker/{job_id}/credentials", get(credentials_handler))
        .route("/worker/{job_id}/prompt", get(prompt_handler))
        .route("/worker/{job_id}/status", post(status_handler))
        .route("/worker/{job_id}/complete", post(complete_handler))
        .route("/worker/{job_id}/event", post(event_handler))
        .with_state(state);

    let (shutdown_tx, shutdown_rx) = oneshot::channel();
    let handle = tokio::spawn(async move {
        axum::serve(listener, app)
            .with_graceful_shutdown(async move {
                let _ = shutdown_rx.await;
            })
            .await
    });
    Ok((format!("http://{}", addr), shutdown_tx, handle))
}

fn build_test_runtime(orchestrator_url: String, job_id: Uuid) -> WorkerRuntime {
    let client = Arc::new(WorkerHttpClient::new(
        orchestrator_url.clone(),
        job_id,
        "test-token".to_string(),
    ));
    WorkerRuntime::from_client(
        WorkerConfig {
            job_id,
            orchestrator_url,
            ..WorkerConfig::default()
        },
        client,
    )
}

struct RuntimeTestHarness {
    runtime: Option<WorkerRuntime>,
    shutdown_tx: Option<oneshot::Sender<()>>,
    handle: Option<tokio::task::JoinHandle<std::io::Result<()>>>,
}

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

async fn setup_runtime_test(
    state: Arc<RuntimeTestState>,
    job_id: Uuid,
) -> std::io::Result<RuntimeTestHarness> {
    let (orchestrator_url, shutdown_tx, handle) =
        spawn_runtime_test_server(Arc::clone(&state)).await?;
    let runtime = build_test_runtime(orchestrator_url, job_id);
    Ok(RuntimeTestHarness {
        runtime: Some(runtime),
        shutdown_tx: Some(shutdown_tx),
        handle: Some(handle),
    })
}

impl RuntimeTestHarness {
    fn take_runtime(&mut self) -> WorkerRuntime {
        self.runtime
            .take()
            .expect("runtime test harness should contain a runtime")
    }

    async fn shutdown_handle(
        handle: tokio::task::JoinHandle<std::io::Result<()>>,
    ) -> std::io::Result<()> {
        handle.await??;
        Ok(())
    }
}

impl Drop for RuntimeTestHarness {
    fn drop(&mut self) {
        if let Some(shutdown_tx) = self.shutdown_tx.take() {
            let _ = shutdown_tx.send(());
        }

        if let Some(handle) = self.handle.take() {
            if let Ok(runtime_handle) = tokio::runtime::Handle::try_current() {
                runtime_handle.spawn(async move {
                    if let Err(error) = RuntimeTestHarness::shutdown_handle(handle).await {
                        tracing::warn!(%error, "runtime test server shutdown failed");
                    }
                });
            } else {
                handle.abort();
            }
        }
    }
}

#[rstest]
#[case(PreLoopFailureCase::GetJob)]
#[case(PreLoopFailureCase::HydrateCredentials)]
#[tokio::test]
async fn worker_runtime_reports_failed_status_for_pre_loop_errors(
    #[case] case: PreLoopFailureCase,
) -> anyhow::Result<()> {
    let state = Arc::new(RuntimeTestState::default());
    match case {
        PreLoopFailureCase::GetJob => {
            state
                .job_statuses
                .lock()
                .await
                .push_back(StatusCode::INTERNAL_SERVER_ERROR);
            state.status_statuses.lock().await.push_back(StatusCode::OK);
        }
        PreLoopFailureCase::HydrateCredentials => {
            state.job_statuses.lock().await.push_back(StatusCode::OK);
            state
                .credential_statuses
                .lock()
                .await
                .push_back(StatusCode::INTERNAL_SERVER_ERROR);
            state.status_statuses.lock().await.push_back(StatusCode::OK);
        }
    }

    let job_id = Uuid::new_v4();
    let mut harness = setup_runtime_test(Arc::clone(&state), job_id).await?;

    let error = harness
        .take_runtime()
        .run()
        .await
        .expect_err("expected runtime to fail before the execution loop");
    assert!(
        !error.to_string().is_empty(),
        "pre-loop failure should preserve the original error"
    );

    let statuses = state.statuses.lock().await;
    assert_eq!(
        statuses.len(),
        1,
        "expected exactly one terminal status update, got {statuses:?}"
    );
    let failed_status = statuses
        .first()
        .filter(|status| status.state == WorkerState::Failed)
        .expect("expected a terminal failed status update");
    assert_eq!(failed_status.iteration, 100);
    assert_eq!(
        failed_status.message.as_deref(),
        Some("pre-loop failure"),
        "expected a sanitised pre-loop failure message, got {failed_status:?}"
    );

    let completions = state.completions.lock().await;
    assert_eq!(
        completions.len(),
        1,
        "expected a terminal completion report"
    );
    assert_eq!(
        completions[0].message.as_deref(),
        Some("Worker failed during startup")
    );
    drop(completions);

    let result_events = state.result_events.lock().await;
    assert_eq!(result_events.len(), 1, "expected a terminal result event");
    assert_eq!(result_events[0]["message"], "Worker failed during startup");
    assert_eq!(result_events[0]["success"], false);

    Ok(())
}

#[tokio::test]
async fn worker_runtime_emits_failed_status_for_initial_status_rejections() -> anyhow::Result<()> {
    let state = Arc::new(RuntimeTestState::default());
    state.job_statuses.lock().await.push_back(StatusCode::OK);
    state
        .status_statuses
        .lock()
        .await
        .push_back(StatusCode::INTERNAL_SERVER_ERROR);
    state.status_statuses.lock().await.push_back(StatusCode::OK);

    let job_id = Uuid::new_v4();
    let mut harness = setup_runtime_test(Arc::clone(&state), job_id).await?;

    let error = harness.take_runtime().run().await;
    let error = error.expect_err("expected runtime to fail when the initial status is rejected");

    assert!(
        matches!(error, WorkerError::OrchestratorRejected { .. }),
        "expected initial status rejection error, got {error}"
    );

    let statuses = state.statuses.lock().await;
    assert_eq!(
        statuses.len(),
        2,
        "expected rejected status + terminal failed status"
    );
    assert_eq!(statuses[0].state, WorkerState::InProgress);
    assert_eq!(statuses[1].state, WorkerState::Failed);
    assert_eq!(statuses[1].iteration, 100);
    assert_eq!(
        statuses[1].message.as_deref(),
        Some("pre-loop failure"),
        "expected a sanitised pre-loop failure status payload, got {:?}",
        statuses[1]
    );

    let completions = state.completions.lock().await;
    assert_eq!(
        completions.len(),
        1,
        "expected a terminal completion report"
    );
    assert_eq!(
        completions[0].message.as_deref(),
        Some("Worker failed during startup")
    );
    drop(completions);

    let result_events = state.result_events.lock().await;
    assert_eq!(result_events.len(), 1, "expected a terminal result event");
    assert_eq!(result_events[0]["message"], "Worker failed during startup");
    assert_eq!(result_events[0]["success"], false);

    Ok(())
}

#[rstest]
#[case(
    WorkerExecutionResult::Failed(Error::Tool(ToolError::ExecutionFailed {
        name: "shell".to_string(),
        reason: "token secret-123 leaked".to_string(),
    })),
    "Execution failed"
)]
#[case(WorkerExecutionResult::TimedOut, "Execution timed out")]
#[tokio::test]
async fn worker_runtime_sanitizes_failure_messages(
    #[case] execution: WorkerExecutionResult,
    #[case] expected_message: &str,
) -> anyhow::Result<()> {
    let state = Arc::new(RuntimeTestState::default());
    let job_id = Uuid::new_v4();
    let harness = setup_runtime_test(Arc::clone(&state), job_id).await?;

    harness
        .runtime
        .as_ref()
        .expect("runtime test harness should contain a runtime")
        .report_completion(execution, 7)
        .await
        .expect("report_completion should succeed in test harness");

    let completions = state.completions.lock().await;
    assert_eq!(completions.len(), 1);
    assert_eq!(completions[0].message.as_deref(), Some(expected_message));
    assert_eq!(completions[0].iterations, 7);
    drop(completions);

    let result_events = state.result_events.lock().await;
    assert_eq!(result_events.len(), 1);
    assert_eq!(result_events[0]["message"], expected_message);
    assert_eq!(result_events[0]["success"], false);
    assert!(
        result_events[0].to_string().contains(expected_message),
        "expected result payload to contain the sanitised message"
    );
    assert!(
        !result_events[0].to_string().contains("secret-123"),
        "result payload should not leak the detailed error text"
    );

    Ok(())
}

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

async fn remote_tool_catalog_with_hosted_guidance(
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

async fn spawn_hosted_guidance_catalog_server() -> (String, tokio::task::JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind listener");
    let addr = listener.local_addr().expect("listener addr");
    let router = Router::new()
        .route(
            REMOTE_TOOL_CATALOG_ROUTE,
            get(remote_tool_catalog_with_hosted_guidance),
        )
        .with_state(TestState);
    let server = tokio::spawn(async move {
        axum::serve(listener, router).await.expect("serve router");
    });
    (format!("http://{addr}"), server)
}

async fn build_runtime_with_remote_tools(base_url: &str) -> (WorkerRuntime, Arc<WorkerHttpClient>) {
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
    runtime.toolset_instructions = runtime
        .register_remote_tools()
        .await
        .expect("register hosted remote tools");
    (runtime, client)
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
}

#[rstest]
#[tokio::test]
async fn worker_runtime_build_reasoning_context_merges_local_and_remote_tools() {
    let (base_url, server) = spawn_hosted_guidance_catalog_server().await;
    let (runtime, _client) = build_runtime_with_remote_tools(&base_url).await;

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
        .find(|message| message.content.contains("Hosted remote-tool guidance"))
        .expect("expected hosted remote-tool guidance message");

    assert!(
        guidance_message
            .content
            .contains("Prefer hosted remote tools for external systems."),
        "reasoning context should include the preserved orchestrator guidance"
    );
    assert_eq!(
        reason_ctx
            .available_tools
            .iter()
            .map(|tool| tool.name.clone())
            .collect::<Vec<_>>(),
        expected_merged_tool_names()
    );

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
}

#[rstest]
#[tokio::test]
async fn worker_runtime_refresh_keeps_merged_tools_without_duplicate_guidance() {
    let (base_url, server) = spawn_hosted_guidance_catalog_server().await;
    let (runtime, client) = build_runtime_with_remote_tools(&base_url).await;

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
        .filter(|message| message.content.contains("Hosted remote-tool guidance"))
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

    assert_eq!(
        reason_ctx
            .available_tools
            .iter()
            .map(|tool| tool.name.clone())
            .collect::<Vec<_>>(),
        expected_merged_tool_names()
    );
    assert_eq!(
        reason_ctx
            .messages
            .iter()
            .filter(|message| message.content.contains("Hosted remote-tool guidance"))
            .count(),
        1,
        "refresh should not duplicate hosted remote-tool guidance"
    );

    server.abort();
    let _ = server.await;
}

#[rstest]
#[tokio::test]
async fn hosted_worker_remote_tool_catalog_degraded_startup_keeps_local_tools() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind listener");
    let addr = listener.local_addr().expect("listener addr");
    let router = Router::new()
        .route(REMOTE_TOOL_CATALOG_ROUTE, get(remote_tool_catalog_error))
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

    runtime.register_remote_tools_with_degraded_startup().await;

    let reason_ctx = runtime
        .build_reasoning_context(&JobDescription {
            title: "Degraded startup".to_string(),
            description: "Continue with local tools only".to_string(),
            project_dir: None,
        })
        .await;

    assert_eq!(
        reason_ctx
            .available_tools
            .iter()
            .map(|tool| tool.name.clone())
            .collect::<Vec<_>>(),
        expected_local_tool_names()
    );
    assert!(
        reason_ctx
            .messages
            .iter()
            .all(|message| !message.content.contains("Hosted remote-tool guidance")),
        "degraded startup should not inject hosted guidance"
    );

    server.abort();
    let _ = server.await;
}
