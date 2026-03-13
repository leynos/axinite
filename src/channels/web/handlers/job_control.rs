//! Job lifecycle, prompt, and event-history handlers.

use std::sync::Arc;

use axum::{
    Json, Router,
    extract::{Path, State},
    http::StatusCode,
    routing::{get, post},
};
use uuid::Uuid;

use crate::channels::web::server::GatewayState;
use crate::db::Database;

mod events;
mod prompt;

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

pub(super) fn internal_error(
    context: &'static str,
    error: impl std::fmt::Display,
) -> (StatusCode, String) {
    tracing::error!(error = %error, "{context}");
    (StatusCode::INTERNAL_SERVER_ERROR, context.to_string())
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

fn strip_retry_prefix(value: &str) -> &str {
    value
        .strip_prefix("Previous attempt failed: ")
        .and_then(|rest| rest.split_once(". Retry: ").map(|(_, base)| base))
        .unwrap_or(value)
}

fn retry_label(base: &str, failure_reason: &str) -> String {
    let base = strip_retry_prefix(base);
    if failure_reason.is_empty() {
        base.to_string()
    } else {
        format!("Previous attempt failed: {failure_reason}. Retry: {base}")
    }
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

async fn mark_sandbox_restart_failed(
    store: &Arc<dyn Database>,
    job_id: Uuid,
    message: String,
) -> Result<(), (StatusCode, String)> {
    store
        .update_sandbox_job_status(
            job_id,
            "failed",
            Some(false),
            Some(&message),
            None,
            Some(chrono::Utc::now()),
        )
        .await
        .map_err(|e| internal_error("Failed to update sandbox job status", e))
}

async fn cancel_sandbox_job(
    state: &GatewayState,
    store: &Arc<dyn Database>,
    job_id: Uuid,
) -> Result<(), (StatusCode, String)> {
    let job_manager = state.job_manager.as_ref().ok_or((
        StatusCode::SERVICE_UNAVAILABLE,
        "Sandbox job manager not available".to_string(),
    ))?;
    job_manager
        .stop_job(job_id)
        .await
        .map_err(|e| internal_error("Failed to stop sandbox job", e))?;
    store
        .update_sandbox_job_status(
            job_id,
            "failed",
            Some(false),
            Some("Cancelled by user"),
            None,
            Some(chrono::Utc::now()),
        )
        .await
        .map_err(|e| internal_error("Failed to update sandbox job status", e))
}

async fn cancel_agent_job(
    state: &GatewayState,
    store: &Arc<dyn Database>,
    job_id: Uuid,
) -> Result<(), (StatusCode, String)> {
    let scheduler_slot = state.scheduler.as_ref().ok_or((
        StatusCode::SERVICE_UNAVAILABLE,
        "Scheduler not available".to_string(),
    ))?;
    let scheduler = {
        let scheduler_guard = scheduler_slot.read().await;
        scheduler_guard.as_ref().cloned().ok_or((
            StatusCode::SERVICE_UNAVAILABLE,
            "Agent scheduler not started".to_string(),
        ))?
    };
    scheduler
        .stop(job_id)
        .await
        .map_err(|e| internal_error("Failed to stop agent job", e))?;
    store
        .update_job_status(
            job_id,
            crate::context::JobState::Cancelled,
            Some("Cancelled by user"),
        )
        .await
        .map_err(|e| internal_error("Failed to update agent job status", e))
}

async fn restart_sandbox_job(
    state: &GatewayState,
    store: &Arc<dyn Database>,
    old_job_id: Uuid,
    old_job: LoadedSandboxJob,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    if !matches!(
        old_job.status,
        SandboxJobStatus::Interrupted | SandboxJobStatus::Failed
    ) {
        return Err((
            StatusCode::CONFLICT,
            format!("Cannot restart job in state '{}'", old_job.record.status),
        ));
    }

    let job_manager = state.job_manager.as_ref().ok_or((
        StatusCode::SERVICE_UNAVAILABLE,
        "Sandbox not enabled".to_string(),
    ))?;

    let base_task = strip_retry_prefix(&old_job.record.task).to_string();
    let task = retry_label(
        &base_task,
        old_job.record.failure_reason.as_deref().unwrap_or(""),
    );

    let new_job_id = Uuid::new_v4();
    let now = chrono::Utc::now();
    let mode = match load_sandbox_job_mode(store, old_job_id).await? {
        Some(SandboxJobMode::ClaudeCode) => crate::orchestrator::job_manager::JobMode::ClaudeCode,
        _ => crate::orchestrator::job_manager::JobMode::Worker,
    };

    let record = crate::history::SandboxJobRecord {
        id: new_job_id,
        task: base_task.clone(),
        status: "creating".to_string(),
        user_id: old_job.record.user_id.clone(),
        project_dir: old_job.record.project_dir.clone(),
        success: None,
        failure_reason: None,
        created_at: now,
        started_at: None,
        completed_at: None,
        credential_grants_json: old_job.record.credential_grants_json.clone(),
    };
    store
        .save_sandbox_job(&record)
        .await
        .map_err(|e| internal_error("Failed to save restarted sandbox job", e))?;

    let credential_grants: Vec<crate::orchestrator::auth::CredentialGrant> =
        serde_json::from_str(&old_job.record.credential_grants_json).unwrap_or_else(|e| {
            tracing::warn!(
                job_id = %old_job.record.id,
                "Failed to deserialize credential grants from stored job: {}. Restarted job will have no credentials.",
                e
            );
            vec![]
        });

    let project_dir = std::path::PathBuf::from(&old_job.record.project_dir);
    if let Err(error) = job_manager
        .create_job(
            new_job_id,
            &task,
            Some(project_dir),
            mode,
            credential_grants,
        )
        .await
    {
        mark_sandbox_restart_failed(store, new_job_id, "Failed to create container".to_string())
            .await?;
        return Err(internal_error("Failed to create container", error));
    }

    if let Err(error) = store
        .update_sandbox_job_status(new_job_id, "running", None, None, Some(now), None)
        .await
    {
        if let Err(stop_error) = job_manager.stop_job(new_job_id).await {
            tracing::error!(
                %error,
                stop_error = %stop_error,
                job_id = %new_job_id,
                "Failed to persist running sandbox state and stop restarted sandbox job"
            );
            return Err((
                StatusCode::INTERNAL_SERVER_ERROR,
                "Failed to persist running sandbox state".to_string(),
            ));
        }
        mark_sandbox_restart_failed(
            store,
            new_job_id,
            "Failed to persist running sandbox state".to_string(),
        )
        .await?;
        return Err(internal_error(
            "Failed to persist running sandbox state",
            error,
        ));
    }

    Ok(Json(serde_json::json!({
        "status": "restarted",
        "old_job_id": old_job_id,
        "new_job_id": new_job_id,
    })))
}

async fn restart_agent_job(
    state: &GatewayState,
    store: &Arc<dyn Database>,
    old_job_id: Uuid,
    old_job: crate::context::JobContext,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    if old_job.state.is_active() {
        return Err((
            StatusCode::CONFLICT,
            format!("Cannot restart job in state '{}'", old_job.state),
        ));
    }

    let slot = state.scheduler.as_ref().ok_or((
        StatusCode::SERVICE_UNAVAILABLE,
        "Scheduler not available".to_string(),
    ))?;
    let scheduler = {
        let scheduler_guard = slot.read().await;
        scheduler_guard.as_ref().cloned().ok_or((
            StatusCode::SERVICE_UNAVAILABLE,
            "Agent not started yet".to_string(),
        ))?
    };

    let failure_reason = store
        .get_agent_job_failure_reason(old_job_id)
        .await
        .map_err(|e| internal_error("Failed to load agent job failure reason", e))?
        .unwrap_or_default();

    let title = retry_label(&old_job.title, &failure_reason);

    let new_job_id = scheduler
        .dispatch_job(&old_job.user_id, &title, &old_job.description, None)
        .await
        .map_err(|e| internal_error("Failed to restart agent job", e))?;

    Ok(Json(serde_json::json!({
        "status": "restarted",
        "old_job_id": old_job_id,
        "new_job_id": new_job_id,
    })))
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
