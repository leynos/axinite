//! Restart helpers for web job-control handlers.

use std::sync::Arc;

use axum::{Json, http::StatusCode};
use uuid::Uuid;

use crate::channels::web::server::GatewayState;
use crate::db::{Database, SandboxJobStatusUpdate};

use super::{LoadedSandboxJob, SandboxJobStatus, internal_error, load_sandbox_job_mode};

/// Mark a sandbox job as running by updating its status with the provided timestamp.
async fn mark_running(
    db: &dyn Database,
    id: Uuid,
    now: chrono::DateTime<chrono::Utc>,
) -> Result<(), crate::error::DatabaseError> {
    db.update_sandbox_job_status(SandboxJobStatusUpdate {
        id,
        status: crate::db::SandboxJobStatus::from("running"),
        success: None,
        message: None,
        started_at: Some(now),
        completed_at: None,
    })
    .await
}

/// Build a standard restart success response.
fn ok_restart_response(
    old_job_id: Uuid,
    new_job_id: Uuid,
) -> (StatusCode, Json<serde_json::Value>) {
    (
        StatusCode::OK,
        Json(serde_json::json!({
            "status": "restarted",
            "old_job_id": old_job_id,
            "new_job_id": new_job_id,
        })),
    )
}

/// Build a new sandbox job record for a restart operation.
fn build_restart_record(
    old_job: &crate::history::SandboxJobRecord,
    new_job_id: Uuid,
    base_task: String,
    now: chrono::DateTime<chrono::Utc>,
) -> crate::history::SandboxJobRecord {
    crate::history::SandboxJobRecord {
        id: new_job_id,
        task: base_task,
        status: "creating".to_string(),
        user_id: old_job.user_id.clone(),
        project_dir: old_job.project_dir.clone(),
        success: None,
        failure_reason: None,
        created_at: now,
        started_at: None,
        completed_at: None,
        credential_grants_json: old_job.credential_grants_json.clone(),
    }
}

/// Parse credential grants from JSON, logging a warning on failure.
fn parse_credential_grants(
    job_id: Uuid,
    json: &str,
) -> Vec<crate::orchestrator::auth::CredentialGrant> {
    serde_json::from_str(json).unwrap_or_else(|e| {
        tracing::warn!(
            job_id = %job_id,
            "Failed to deserialize credential grants from stored job: {}. Restarted job will have no credentials.",
            e
        );
        vec![]
    })
}

/// Handle mark_running failure by stopping the job and updating status.
async fn handle_mark_running_failure(
    store: &Arc<dyn Database>,
    job_manager: &crate::orchestrator::job_manager::ContainerJobManager,
    new_job_id: Uuid,
    error: crate::error::DatabaseError,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
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
    Err(internal_error(
        "Failed to persist running sandbox state",
        error,
    ))
}

/// Persist a restarted sandbox job record and optionally its mode.
async fn persist_restart_job(
    store: &Arc<dyn Database>,
    old_job: &crate::history::SandboxJobRecord,
    new_job_id: Uuid,
    base_task: String,
    now: chrono::DateTime<chrono::Utc>,
    mode: crate::orchestrator::job_manager::JobMode,
) -> Result<(), (StatusCode, String)> {
    let record = build_restart_record(old_job, new_job_id, base_task, now);
    store
        .save_sandbox_job(&record)
        .await
        .map_err(|e| internal_error("Failed to save restarted sandbox job", e))?;

    // Persist the job mode if it's ClaudeCode.
    //
    // Invariant: Worker is the default mode; only non-default modes (e.g.,
    // JobMode::ClaudeCode) are persisted to the DB. NULL job_mode implies Worker.
    // The is_claude_code boolean tracks whether we need to persist
    // SandboxMode::ClaudeCode. The load path (load_sandbox_job_mode) and API layer
    // convert NULL to Worker. This conditional must preserve that behaviour:
    // only persist when mode is explicitly ClaudeCode.
    //
    // Note: The two-step persistence (save_sandbox_job then update_sandbox_job_mode)
    // is not transactional and may leave a NULL mode if the update fails, but this
    // is safe due to the Worker default on load.
    let is_claude_code = mode == crate::orchestrator::job_manager::JobMode::ClaudeCode;
    if is_claude_code
        && let Err(error) = store
            .update_sandbox_job_mode(new_job_id, crate::db::SandboxMode::ClaudeCode)
            .await
    {
        mark_sandbox_restart_failed(
            store,
            new_job_id,
            "Failed to persist restarted sandbox job mode".to_string(),
        )
        .await?;
        return Err(internal_error(
            "Failed to persist restarted sandbox job mode",
            error,
        ));
    }

    Ok(())
}

/// Create a container for a restarted sandbox job.
async fn create_restart_container(
    store: &Arc<dyn Database>,
    job_manager: &crate::orchestrator::job_manager::ContainerJobManager,
    old_job: &crate::history::SandboxJobRecord,
    new_job_id: Uuid,
    task: &str,
    mode: crate::orchestrator::job_manager::JobMode,
) -> Result<(), (StatusCode, String)> {
    let credential_grants = parse_credential_grants(old_job.id, &old_job.credential_grants_json);

    let project_dir = std::path::PathBuf::from(&old_job.project_dir);
    if let Err(error) = job_manager
        .create_job(new_job_id, task, Some(project_dir), mode, credential_grants)
        .await
    {
        mark_sandbox_restart_failed(store, new_job_id, "Failed to create container".to_string())
            .await?;
        return Err(internal_error("Failed to create container", error));
    }

    Ok(())
}

pub(super) async fn restart_sandbox_job(
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
        Some(crate::db::SandboxMode::ClaudeCode) => {
            crate::orchestrator::job_manager::JobMode::ClaudeCode
        }
        _ => crate::orchestrator::job_manager::JobMode::Worker,
    };

    persist_restart_job(store, &old_job.record, new_job_id, base_task, now, mode).await?;
    create_restart_container(store, job_manager, &old_job.record, new_job_id, &task, mode).await?;

    if let Err(error) = mark_running(&**store, new_job_id, now).await {
        return handle_mark_running_failure(store, job_manager, new_job_id, error).await;
    }

    let (_status, json) = ok_restart_response(old_job_id, new_job_id);
    Ok(json)
}

pub(super) async fn restart_agent_job(
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

async fn mark_sandbox_restart_failed(
    store: &Arc<dyn Database>,
    job_id: Uuid,
    message: String,
) -> Result<(), (StatusCode, String)> {
    store
        .update_sandbox_job_status(SandboxJobStatusUpdate {
            id: job_id,
            status: crate::db::SandboxJobStatus::from("failed"),
            success: Some(false),
            message: Some(&message),
            started_at: None,
            completed_at: Some(chrono::Utc::now()),
        })
        .await
        .map_err(|e| internal_error("Failed to update sandbox job status", e))
}
