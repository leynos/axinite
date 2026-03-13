//! Event-history handlers for jobs.

use std::sync::Arc;

use axum::{
    Json,
    extract::{Path, Query, State},
    http::StatusCode,
};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::channels::web::server::GatewayState;

const DEFAULT_EVENT_LIMIT: usize = 50;
const MAX_EVENT_LIMIT: usize = 200;

#[derive(Debug, Deserialize, Default)]
pub(super) struct JobEventsQuery {
    limit: Option<usize>,
    before_id: Option<i64>,
}

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
    next_before_id: Option<i64>,
}

pub async fn jobs_events_handler(
    State(state): State<Arc<GatewayState>>,
    Path(id): Path<String>,
    Query(query): Query<JobEventsQuery>,
) -> Result<Json<JobEventsResponse>, (StatusCode, String)> {
    let store = state
        .store
        .as_ref()
        .ok_or_else(super::database_unavailable)?;

    let job_id: Uuid = id
        .parse()
        .map_err(|_| (StatusCode::BAD_REQUEST, "Invalid job ID".to_string()))?;

    if let Some(before_id) = query.before_id
        && before_id <= 0
    {
        return Err((
            StatusCode::BAD_REQUEST,
            "'before_id' must be a positive integer".to_string(),
        ));
    }

    let limit = query
        .limit
        .unwrap_or(DEFAULT_EVENT_LIMIT)
        .clamp(1, MAX_EVENT_LIMIT);
    let fetch_limit = limit.saturating_add(1) as i64;

    let events = store
        .list_job_events(job_id, query.before_id, Some(fetch_limit))
        .await
        .map_err(|e| super::internal_error("Failed to load job events", e))?;

    let mut events = events;
    let next_before_id = if events.len() > limit {
        events.remove(0);
        events.first().map(|event| event.id)
    } else {
        None
    };

    let events = events
        .into_iter()
        .map(|event| JobEventResponse {
            id: event.id,
            event_type: event.event_type,
            data: event.data,
            created_at: event.created_at.to_rfc3339(),
        })
        .collect();

    Ok(Json(JobEventsResponse {
        job_id,
        events,
        next_before_id,
    }))
}
