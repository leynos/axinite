//! Tests for container delegate shutdown behaviour.

use std::sync::Arc;

use anyhow::Result;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::routing::post;
use axum::{Json, Router};
use tokio::net::TcpListener;
use tokio::sync::{Mutex, Notify};
use uuid::Uuid;

use crate::worker::api::{EVENT_ROUTE, JobEventPayload, WorkerHttpClient};
use crate::worker::container::delegate::ContainerDelegate;

use super::test_support::build_test_runtime;

#[derive(Default)]
struct EventState {
    events: Mutex<Vec<JobEventPayload>>,
    notify: Notify,
}

async fn event_handler(
    State(state): State<Arc<EventState>>,
    Path(_job_id): Path<Uuid>,
    Json(payload): Json<JobEventPayload>,
) -> StatusCode {
    state.events.lock().await.push(payload);
    state.notify.notify_waiters();
    StatusCode::OK
}

async fn spawn_event_server(
    state: Arc<EventState>,
) -> Result<(String, tokio::task::JoinHandle<()>)> {
    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let addr = listener.local_addr()?;
    let app = Router::new()
        .route(EVENT_ROUTE, post(event_handler))
        .with_state(state);
    let handle = tokio::spawn(async move {
        axum::serve(listener, app)
            .await
            .expect("event test server should run");
    });
    Ok((format!("http://{addr}"), handle))
}

#[tokio::test]
async fn container_delegate_shutdown_drains_buffered_events() -> Result<()> {
    let state = Arc::new(EventState::default());
    let (base_url, handle) = spawn_event_server(Arc::clone(&state)).await?;
    let runtime = build_test_runtime(base_url.clone(), Uuid::nil())?;
    let client = Arc::new(WorkerHttpClient::new(
        base_url,
        Uuid::nil(),
        "test-token".to_string(),
    )?);

    let delegate = ContainerDelegate::new(
        client,
        Arc::clone(&runtime.safety),
        Arc::clone(&runtime.tools),
        Arc::clone(&runtime.extra_env),
        Arc::new(Mutex::new(0)),
    );

    delegate.post_event(
        crate::worker::api::JobEventType::Message,
        serde_json::json!({"content": "queued"}),
    );
    delegate.shutdown().await;

    tokio::time::timeout(std::time::Duration::from_secs(1), async {
        loop {
            if state.events.lock().await.len() == 1 {
                break;
            }
            state.notify.notified().await;
        }
    })
    .await?;
    let events = state.events.lock().await;
    assert_eq!(events.len(), 1, "shutdown should flush the queued event");
    assert_eq!(events[0].data["content"], "queued");

    handle.abort();
    let _ = handle.await;
    Ok(())
}
