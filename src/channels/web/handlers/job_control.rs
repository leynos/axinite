//! Job lifecycle, prompt, and event-history handlers.

use std::sync::Arc;

use axum::{
    Json, Router,
    extract::{Path, State},
    http::StatusCode,
    routing::{get, post},
};
use uuid::Uuid;

use crate::channels::web::handlers::common::internal_error;
use crate::channels::web::server::GatewayState;
use crate::db::Database;

mod cancel;
mod events;
mod prompt;
mod restart;

use cancel::{cancel_agent_job, cancel_sandbox_job};
use restart::{restart_agent_job, restart_sandbox_job};

pub fn routes() -> Router<Arc<GatewayState>> {
    Router::new()
        .route("/api/jobs/{id}/cancel", post(jobs_cancel_handler))
        .route("/api/jobs/{id}/restart", post(jobs_restart_handler))
        .route("/api/jobs/{id}/prompt", post(prompt::jobs_prompt_handler))
        .route("/api/jobs/{id}/events", get(events::jobs_events_handler))
}

fn database_unavailable() -> (StatusCode, String) {
    (
        StatusCode::SERVICE_UNAVAILABLE,
        "Database not available".to_string(),
    )
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum SandboxJobStatus {
    Creating,
    Running,
    Failed,
    Interrupted,
}

impl TryFrom<&str> for SandboxJobStatus {
    type Error = String;

    fn try_from(value: &str) -> Result<Self, Self::Error> {
        match value {
            "creating" => Ok(Self::Creating),
            "running" => Ok(Self::Running),
            "failed" => Ok(Self::Failed),
            "interrupted" => Ok(Self::Interrupted),
            other => Err(format!("unexpected sandbox job status '{other}'")),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum SandboxJobMode {
    Worker,
    ClaudeCode,
}

impl TryFrom<&str> for SandboxJobMode {
    type Error = String;

    fn try_from(value: &str) -> Result<Self, Self::Error> {
        match value {
            "worker" => Ok(Self::Worker),
            "claude_code" => Ok(Self::ClaudeCode),
            other => Err(format!("unexpected sandbox job mode '{other}'")),
        }
    }
}

pub(super) struct LoadedSandboxJob {
    pub(super) record: crate::history::SandboxJobRecord,
    pub(super) status: SandboxJobStatus,
}

fn sandbox_job_accepts_prompts(status: SandboxJobStatus) -> bool {
    matches!(
        status,
        SandboxJobStatus::Creating | SandboxJobStatus::Running
    )
}

async fn load_sandbox_job(
    store: &Arc<dyn Database>,
    job_id: Uuid,
) -> Result<Option<LoadedSandboxJob>, (StatusCode, String)> {
    let record = store
        .get_sandbox_job(job_id)
        .await
        .map_err(|e| internal_error("Failed to load sandbox job", e))?;
    record
        .map(|record| {
            let status = SandboxJobStatus::try_from(record.status.as_str())
                .map_err(|e| internal_error("Failed to parse sandbox job status", e))?;
            Ok(LoadedSandboxJob { record, status })
        })
        .transpose()
}

async fn load_agent_job(
    store: &Arc<dyn Database>,
    job_id: Uuid,
) -> Result<Option<crate::context::JobContext>, (StatusCode, String)> {
    store
        .get_job(job_id)
        .await
        .map_err(|e| internal_error("Failed to load agent job", e))
}

async fn load_sandbox_job_mode(
    store: &Arc<dyn Database>,
    job_id: Uuid,
) -> Result<Option<SandboxJobMode>, (StatusCode, String)> {
    let mode = store
        .get_sandbox_job_mode(job_id)
        .await
        .map_err(|e| internal_error("Failed to load sandbox job mode", e))?;
    mode.map(|mode| {
        SandboxJobMode::try_from(mode.as_str())
            .map_err(|e| internal_error("Failed to parse sandbox job mode", e))
    })
    .transpose()
}

pub async fn jobs_cancel_handler(
    State(state): State<Arc<GatewayState>>,
    Path(id): Path<String>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    let store = state.store.as_ref().ok_or_else(database_unavailable)?;
    let job_id = Uuid::parse_str(&id)
        .map_err(|_| (StatusCode::BAD_REQUEST, "Invalid job ID".to_string()))?;

    if let Some(job) = load_sandbox_job(store, job_id).await? {
        if matches!(
            job.status,
            SandboxJobStatus::Running | SandboxJobStatus::Creating
        ) {
            cancel_sandbox_job(state.as_ref(), store, job_id).await?;
        }
        return Ok(Json(serde_json::json!({
            "status": "cancelled",
            "job_id": job_id,
        })));
    }

    if let Some(job) = load_agent_job(store, job_id).await? {
        if job.state.is_active() {
            cancel_agent_job(state.as_ref(), store, job_id).await?;
        }
        return Ok(Json(serde_json::json!({
            "status": "cancelled",
            "job_id": job_id,
        })));
    }

    Err((StatusCode::NOT_FOUND, "Job not found".to_string()))
}

pub async fn jobs_restart_handler(
    State(state): State<Arc<GatewayState>>,
    Path(id): Path<String>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    let store = state.store.as_ref().ok_or_else(database_unavailable)?;

    let old_job_id = Uuid::parse_str(&id)
        .map_err(|_| (StatusCode::BAD_REQUEST, "Invalid job ID".to_string()))?;

    if let Some(old_job) = load_sandbox_job(store, old_job_id).await? {
        return restart_sandbox_job(state.as_ref(), store, old_job_id, old_job).await;
    }

    if let Some(old_job) = load_agent_job(store, old_job_id).await? {
        return restart_agent_job(state.as_ref(), store, old_job_id, old_job).await;
    }

    Err((StatusCode::NOT_FOUND, "Job not found".to_string()))
}
