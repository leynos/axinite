//! Event-history handlers for jobs.

use std::sync::Arc;

use axum::{
    Json,
    extract::{Path, State},
    http::StatusCode,
};
use serde::Serialize;
use uuid::Uuid;

use crate::channels::web::server::GatewayState;

#[derive(Serialize)]
pub(super) struct JobEventResponse {
    id: i64,
    event_type: String,
    data: serde_json::Value,
    created_at: String,
}

#[derive(Serialize)]
pub(super) struct JobEventsResponse {
    job_id: Uuid,
    events: Vec<JobEventResponse>,
}

pub async fn jobs_events_handler(
    State(state): State<Arc<GatewayState>>,
    Path(id): Path<String>,
) -> Result<Json<JobEventsResponse>, (StatusCode, String)> {
    let store = state
        .store
        .as_ref()
        .ok_or_else(super::database_unavailable)?;

    let job_id: Uuid = id
        .parse()
        .map_err(|_| (StatusCode::BAD_REQUEST, "Invalid job ID".to_string()))?;

    let events = store
        .list_job_events(job_id, None)
        .await
        .map_err(|e| super::internal_error("Failed to load job events", e))?;

    let events = events
        .into_iter()
        .map(|event| JobEventResponse {
            id: event.id,
            event_type: event.event_type,
            data: event.data,
            created_at: event.created_at.to_rfc3339(),
        })
        .collect();

    Ok(Json(JobEventsResponse { job_id, events }))
}
