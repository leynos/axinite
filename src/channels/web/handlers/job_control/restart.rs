//! Restart helpers for web job-control handlers.

use std::sync::Arc;

use axum::{Json, http::StatusCode};
use uuid::Uuid;

use crate::channels::web::server::GatewayState;
use crate::db::Database;

use super::{LoadedSandboxJob, SandboxJobStatus, internal_error, load_sandbox_job_mode};

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
        Some(super::SandboxJobMode::ClaudeCode) => {
            crate::orchestrator::job_manager::JobMode::ClaudeCode
        }
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
