//! Shared test infrastructure for container worker runtime tests.

use std::collections::VecDeque;
use std::sync::Arc;

use axum::extract::State;
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::routing::{get, post};
use axum::{Json, Router};
use tokio::net::TcpListener;
use tokio::sync::Mutex;
use tokio::sync::oneshot;
use uuid::Uuid;

use crate::worker::api::{CompletionReport, CredentialResponse, JobDescription, StatusUpdate};
use crate::worker::container::{WorkerConfig, WorkerHttpClient, WorkerRuntime};

#[derive(Default)]
pub struct RuntimeTestState {
    pub job_statuses: Mutex<VecDeque<StatusCode>>,
    pub credential_statuses: Mutex<VecDeque<StatusCode>>,
    pub status_statuses: Mutex<VecDeque<StatusCode>>,
    pub statuses: Mutex<Vec<StatusUpdate>>,
    pub completions: Mutex<Vec<CompletionReport>>,
    pub result_events: Mutex<Vec<serde_json::Value>>,
}

pub async fn take_next_status(
    queue: &Mutex<VecDeque<StatusCode>>,
    default: StatusCode,
) -> StatusCode {
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
    Json(serde_json::json!({ "status": "ok" }))
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

pub async fn spawn_runtime_test_server(
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

pub fn build_test_runtime(orchestrator_url: String, job_id: Uuid) -> WorkerRuntime {
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

pub struct RuntimeTestHarness {
    pub runtime: Option<WorkerRuntime>,
    shutdown_tx: Option<oneshot::Sender<()>>,
    handle: Option<tokio::task::JoinHandle<std::io::Result<()>>>,
}

impl RuntimeTestHarness {
    pub fn take_runtime(&mut self) -> WorkerRuntime {
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

pub async fn setup_runtime_test(
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
