//! Tests for `WorkerHttpClient` status, event, prompt, credential, and completion methods.

use std::sync::Arc;

use axum::extract::{Path, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::IntoResponse;
use axum::routing::{get, post};
use axum::{Json, Router};
use tokio::net::TcpListener;
use tokio::sync::Mutex;
use uuid::Uuid;

use crate::testing::credentials::TEST_BEARER_TOKEN;
use crate::worker::api::{
    COMPLETE_ROUTE, CREDENTIALS_ROUTE, CompletionReport, CredentialResponse, EVENT_ROUTE,
    JobEventPayload, PROMPT_ROUTE, STATUS_ROUTE, StatusUpdate, WorkerHttpClient, WorkerState,
};

#[derive(Default)]
struct ClientMethodTestState {
    status_updates: Mutex<Vec<StatusUpdate>>,
    event_payloads: Mutex<Vec<JobEventPayload>>,
    completion_reports: Mutex<Vec<CompletionReport>>,
    auth_headers: Mutex<Vec<String>>,
}

async fn record_auth(headers: &HeaderMap, state: &ClientMethodTestState) {
    let auth = headers
        .get(axum::http::header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .unwrap_or_default()
        .to_string();
    state.auth_headers.lock().await.push(auth);
}

async fn spawn_test_server(
    router: Router,
) -> anyhow::Result<(String, tokio::task::JoinHandle<()>)> {
    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let addr = listener.local_addr()?;
    let handle = tokio::spawn(async move {
        axum::serve(listener, router)
            .await
            .expect("client method test server should run");
    });
    Ok((format!("http://{addr}"), handle))
}

#[tokio::test]
async fn worker_http_client_report_status_posts_status_payload() -> anyhow::Result<()> {
    async fn handler(
        State(state): State<Arc<ClientMethodTestState>>,
        Path(_job_id): Path<Uuid>,
        headers: HeaderMap,
        Json(update): Json<StatusUpdate>,
    ) -> StatusCode {
        record_auth(&headers, &state).await;
        state.status_updates.lock().await.push(update);
        StatusCode::OK
    }

    let state = Arc::new(ClientMethodTestState::default());
    let (base_url, handle) = spawn_test_server(
        Router::new()
            .route(STATUS_ROUTE, post(handler))
            .with_state(Arc::clone(&state)),
    )
    .await?;
    let client = WorkerHttpClient::new(base_url, Uuid::nil(), TEST_BEARER_TOKEN.to_string())?;

    let update = StatusUpdate::new(WorkerState::InProgress, Some("working".to_string()), 3);
    client.report_status(&update).await?;

    let updates = state.status_updates.lock().await;
    assert_eq!(updates.len(), 1);
    assert_eq!(updates[0].state, update.state);
    assert_eq!(updates[0].message, update.message);
    assert_eq!(updates[0].iteration, update.iteration);
    drop(updates);
    assert_eq!(
        state.auth_headers.lock().await.as_slice(),
        &[format!("Bearer {TEST_BEARER_TOKEN}")]
    );

    handle.abort();
    let _ = handle.await;
    Ok(())
}

#[tokio::test]
async fn worker_http_client_report_status_lossy_swallows_rejections() -> anyhow::Result<()> {
    async fn handler(
        State(state): State<Arc<ClientMethodTestState>>,
        Path(_job_id): Path<Uuid>,
        headers: HeaderMap,
        Json(update): Json<StatusUpdate>,
    ) -> (StatusCode, &'static str) {
        record_auth(&headers, &state).await;
        state.status_updates.lock().await.push(update);
        (StatusCode::INTERNAL_SERVER_ERROR, "nope")
    }

    let state = Arc::new(ClientMethodTestState::default());
    let (base_url, handle) = spawn_test_server(
        Router::new()
            .route(STATUS_ROUTE, post(handler))
            .with_state(Arc::clone(&state)),
    )
    .await?;
    let client = WorkerHttpClient::new(base_url, Uuid::nil(), TEST_BEARER_TOKEN.to_string())?;

    let update = StatusUpdate::new(WorkerState::Running, Some("still going".to_string()), 5);
    client.report_status_lossy(&update).await;

    let updates = state.status_updates.lock().await;
    assert_eq!(updates.len(), 1);
    assert_eq!(updates[0].state, update.state);
    assert_eq!(updates[0].message, update.message);
    assert_eq!(updates[0].iteration, update.iteration);

    handle.abort();
    let _ = handle.await;
    Ok(())
}

#[tokio::test]
async fn worker_http_client_post_event_posts_event_payload() -> anyhow::Result<()> {
    async fn handler(
        State(state): State<Arc<ClientMethodTestState>>,
        Path(_job_id): Path<Uuid>,
        headers: HeaderMap,
        Json(payload): Json<JobEventPayload>,
    ) -> StatusCode {
        record_auth(&headers, &state).await;
        state.event_payloads.lock().await.push(payload);
        StatusCode::OK
    }

    let state = Arc::new(ClientMethodTestState::default());
    let (base_url, handle) = spawn_test_server(
        Router::new()
            .route(EVENT_ROUTE, post(handler))
            .with_state(Arc::clone(&state)),
    )
    .await?;
    let client = WorkerHttpClient::new(base_url, Uuid::nil(), TEST_BEARER_TOKEN.to_string())?;
    let payload = JobEventPayload {
        event_type: crate::worker::api::JobEventType::Message,
        data: serde_json::json!({"role": "assistant", "content": "hello"}),
    };

    client.post_event(&payload).await?;

    assert_eq!(state.event_payloads.lock().await.len(), 1);
    assert_eq!(
        state.event_payloads.lock().await[0].data["content"],
        serde_json::json!("hello")
    );

    handle.abort();
    let _ = handle.await;
    Ok(())
}

#[tokio::test]
async fn worker_http_client_poll_prompt_returns_prompt_response() -> anyhow::Result<()> {
    async fn handler(
        State(state): State<Arc<ClientMethodTestState>>,
        Path(_job_id): Path<Uuid>,
        headers: HeaderMap,
    ) -> impl IntoResponse {
        record_auth(&headers, &state).await;
        (
            StatusCode::OK,
            [(axum::http::header::CONTENT_TYPE, "application/json")],
            r#"{"content":"follow up","done":false}"#,
        )
    }

    let state = Arc::new(ClientMethodTestState::default());
    let (base_url, handle) = spawn_test_server(
        Router::new()
            .route(PROMPT_ROUTE, get(handler))
            .with_state(Arc::clone(&state)),
    )
    .await?;
    let client = WorkerHttpClient::new(base_url, Uuid::nil(), TEST_BEARER_TOKEN.to_string())?;

    let prompt = client
        .poll_prompt()
        .await?
        .expect("prompt should be present");

    assert_eq!(prompt.content, "follow up");
    assert!(!prompt.done);

    handle.abort();
    let _ = handle.await;
    Ok(())
}

#[tokio::test]
async fn worker_http_client_poll_prompt_returns_none_for_no_content() -> anyhow::Result<()> {
    async fn handler(
        State(state): State<Arc<ClientMethodTestState>>,
        Path(_job_id): Path<Uuid>,
        headers: HeaderMap,
    ) -> StatusCode {
        record_auth(&headers, &state).await;
        StatusCode::NO_CONTENT
    }

    let state = Arc::new(ClientMethodTestState::default());
    let (base_url, handle) = spawn_test_server(
        Router::new()
            .route(PROMPT_ROUTE, get(handler))
            .with_state(Arc::clone(&state)),
    )
    .await?;
    let client = WorkerHttpClient::new(base_url, Uuid::nil(), TEST_BEARER_TOKEN.to_string())?;

    assert!(client.poll_prompt().await?.is_none());

    handle.abort();
    let _ = handle.await;
    Ok(())
}

#[tokio::test]
async fn worker_http_client_fetch_credentials_returns_payload() -> anyhow::Result<()> {
    async fn handler(
        State(state): State<Arc<ClientMethodTestState>>,
        Path(_job_id): Path<Uuid>,
        headers: HeaderMap,
    ) -> Json<Vec<CredentialResponse>> {
        record_auth(&headers, &state).await;
        Json(vec![CredentialResponse {
            env_var: "API_TOKEN".to_string(),
            value: "secret".to_string(),
        }])
    }

    let state = Arc::new(ClientMethodTestState::default());
    let (base_url, handle) = spawn_test_server(
        Router::new()
            .route(CREDENTIALS_ROUTE, get(handler))
            .with_state(Arc::clone(&state)),
    )
    .await?;
    let client = WorkerHttpClient::new(base_url, Uuid::nil(), TEST_BEARER_TOKEN.to_string())?;

    let credentials = client.fetch_credentials().await?;

    assert_eq!(credentials.len(), 1);
    assert_eq!(credentials[0].env_var, "API_TOKEN");
    assert_eq!(credentials[0].value, "secret");

    handle.abort();
    let _ = handle.await;
    Ok(())
}

#[tokio::test]
async fn worker_http_client_fetch_credentials_returns_empty_for_no_content() -> anyhow::Result<()> {
    async fn handler(
        State(state): State<Arc<ClientMethodTestState>>,
        Path(_job_id): Path<Uuid>,
        headers: HeaderMap,
    ) -> StatusCode {
        record_auth(&headers, &state).await;
        StatusCode::NO_CONTENT
    }

    let state = Arc::new(ClientMethodTestState::default());
    let (base_url, handle) = spawn_test_server(
        Router::new()
            .route(CREDENTIALS_ROUTE, get(handler))
            .with_state(Arc::clone(&state)),
    )
    .await?;
    let client = WorkerHttpClient::new(base_url, Uuid::nil(), TEST_BEARER_TOKEN.to_string())?;

    assert!(client.fetch_credentials().await?.is_empty());

    handle.abort();
    let _ = handle.await;
    Ok(())
}

#[tokio::test]
async fn worker_http_client_report_complete_posts_completion_report() -> anyhow::Result<()> {
    async fn handler(
        State(state): State<Arc<ClientMethodTestState>>,
        Path(_job_id): Path<Uuid>,
        headers: HeaderMap,
        Json(report): Json<CompletionReport>,
    ) -> StatusCode {
        record_auth(&headers, &state).await;
        state.completion_reports.lock().await.push(report);
        StatusCode::OK
    }

    let state = Arc::new(ClientMethodTestState::default());
    let (base_url, handle) = spawn_test_server(
        Router::new()
            .route(COMPLETE_ROUTE, post(handler))
            .with_state(Arc::clone(&state)),
    )
    .await?;
    let client = WorkerHttpClient::new(base_url, Uuid::nil(), TEST_BEARER_TOKEN.to_string())?;
    let report = CompletionReport {
        success: true,
        message: Some("done".to_string()),
        iterations: 9,
    };

    client.report_complete(&report).await?;

    let reports = state.completion_reports.lock().await;
    assert_eq!(reports.len(), 1);
    assert_eq!(reports[0].success, report.success);
    assert_eq!(reports[0].message, report.message);
    assert_eq!(reports[0].iterations, report.iterations);

    handle.abort();
    let _ = handle.await;
    Ok(())
}
