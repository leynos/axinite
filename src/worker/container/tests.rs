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

use super::*;
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
    let names = WorkerRuntime::build_tools().list().await;

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

#[derive(Clone)]
struct TestState;

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
