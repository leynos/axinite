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

pub fn routes() -> Router<Arc<GatewayState>> {
    Router::new()
        .route("/api/jobs/{id}/cancel", post(jobs_cancel_handler))
        .route("/api/jobs/{id}/restart", post(jobs_restart_handler))
        .route("/api/jobs/{id}/prompt", post(jobs_prompt_handler))
        .route("/api/jobs/{id}/events", get(jobs_events_handler))
}

pub async fn jobs_cancel_handler(
    State(state): State<Arc<GatewayState>>,
    Path(id): Path<String>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    let job_id = Uuid::parse_str(&id)
        .map_err(|_| (StatusCode::BAD_REQUEST, "Invalid job ID".to_string()))?;

    if let Some(ref store) = state.store
        && let Ok(Some(job)) = store.get_sandbox_job(job_id).await
    {
        if job.status == "running" || job.status == "creating" {
            if let Some(ref jm) = state.job_manager
                && let Err(e) = jm.stop_job(job_id).await
            {
                tracing::warn!(job_id = %job_id, error = %e, "Failed to stop container during cancellation");
            }
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
                .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
        }
        return Ok(Json(serde_json::json!({
            "status": "cancelled",
            "job_id": job_id,
        })));
    }

    if let Some(ref store) = state.store
        && let Ok(Some(job)) = store.get_job(job_id).await
    {
        if job.state.is_active() {
            if let Some(ref slot) = state.scheduler
                && let Some(ref scheduler) = *slot.read().await
            {
                let _ = scheduler.stop(job_id).await;
            }

            store
                .update_job_status(
                    job_id,
                    crate::context::JobState::Cancelled,
                    Some("Cancelled by user"),
                )
                .await
                .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
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
    let store = state.store.as_ref().ok_or((
        StatusCode::SERVICE_UNAVAILABLE,
        "Database not available".to_string(),
    ))?;

    let old_job_id = Uuid::parse_str(&id)
        .map_err(|_| (StatusCode::BAD_REQUEST, "Invalid job ID".to_string()))?;

    if let Ok(Some(old_job)) = store.get_sandbox_job(old_job_id).await {
        if old_job.status != "interrupted" && old_job.status != "failed" {
            return Err((
                StatusCode::CONFLICT,
                format!("Cannot restart job in state '{}'", old_job.status),
            ));
        }

        let jm = state.job_manager.as_ref().ok_or((
            StatusCode::SERVICE_UNAVAILABLE,
            "Sandbox not enabled".to_string(),
        ))?;

        let task = if let Some(ref reason) = old_job.failure_reason {
            format!(
                "Previous attempt failed: {}. Retry: {}",
                reason, old_job.task
            )
        } else {
            old_job.task.clone()
        };

        let new_job_id = Uuid::new_v4();
        let now = chrono::Utc::now();

        let record = crate::history::SandboxJobRecord {
            id: new_job_id,
            task: task.clone(),
            status: "creating".to_string(),
            user_id: old_job.user_id.clone(),
            project_dir: old_job.project_dir.clone(),
            success: None,
            failure_reason: None,
            created_at: now,
            started_at: None,
            completed_at: None,
            credential_grants_json: old_job.credential_grants_json.clone(),
        };
        store
            .save_sandbox_job(&record)
            .await
            .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

        let mode = match store.get_sandbox_job_mode(old_job_id).await {
            Ok(Some(m)) if m == "claude_code" => {
                crate::orchestrator::job_manager::JobMode::ClaudeCode
            }
            _ => crate::orchestrator::job_manager::JobMode::Worker,
        };

        let credential_grants: Vec<crate::orchestrator::auth::CredentialGrant> =
            serde_json::from_str(&old_job.credential_grants_json).unwrap_or_else(|e| {
                tracing::warn!(
                    job_id = %old_job.id,
                    "Failed to deserialize credential grants from stored job: {}. Restarted job will have no credentials.",
                    e
                );
                vec![]
            });

        let project_dir = std::path::PathBuf::from(&old_job.project_dir);
        let _token = jm
            .create_job(
                new_job_id,
                &task,
                Some(project_dir),
                mode,
                credential_grants,
            )
            .await
            .map_err(|e| {
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    format!("Failed to create container: {}", e),
                )
            })?;

        store
            .update_sandbox_job_status(new_job_id, "running", None, None, Some(now), None)
            .await
            .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

        return Ok(Json(serde_json::json!({
            "status": "restarted",
            "old_job_id": old_job_id,
            "new_job_id": new_job_id,
        })));
    }

    if let Ok(Some(old_job)) = store.get_job(old_job_id).await {
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
        let scheduler_guard = slot.read().await;
        let scheduler = scheduler_guard.as_ref().ok_or((
            StatusCode::SERVICE_UNAVAILABLE,
            "Agent not started yet".to_string(),
        ))?;

        let failure_reason = store
            .get_agent_job_failure_reason(old_job_id)
            .await
            .ok()
            .flatten()
            .unwrap_or_default();

        let title = if !failure_reason.is_empty() {
            format!(
                "Previous attempt failed: {}. Retry: {}",
                failure_reason, old_job.title
            )
        } else {
            old_job.title.clone()
        };

        let new_job_id = scheduler
            .dispatch_job(&old_job.user_id, &title, &old_job.description, None)
            .await
            .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

        return Ok(Json(serde_json::json!({
            "status": "restarted",
            "old_job_id": old_job_id,
            "new_job_id": new_job_id,
        })));
    }

    Err((StatusCode::NOT_FOUND, "Job not found".to_string()))
}

pub async fn jobs_prompt_handler(
    State(state): State<Arc<GatewayState>>,
    Path(id): Path<String>,
    Json(body): Json<serde_json::Value>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    let job_id: uuid::Uuid = id
        .parse()
        .map_err(|_| (StatusCode::BAD_REQUEST, "Invalid job ID".to_string()))?;

    let content = body
        .get("content")
        .and_then(|v| v.as_str())
        .ok_or((
            StatusCode::BAD_REQUEST,
            "Missing 'content' field".to_string(),
        ))?
        .to_string();

    let done = body.get("done").and_then(|v| v.as_bool()).unwrap_or(false);

    if let Some(ref s) = state.store
        && let Ok(Some(_)) = s.get_sandbox_job(job_id).await
    {
        let mode = s.get_sandbox_job_mode(job_id).await.ok().flatten();
        if mode.as_deref() == Some("claude_code") {
            let prompt_queue = state.prompt_queue.as_ref().ok_or((
                StatusCode::NOT_IMPLEMENTED,
                "Claude Code not configured".to_string(),
            ))?;
            let prompt = crate::orchestrator::api::PendingPrompt { content, done };
            {
                let mut queue = prompt_queue.lock().await;
                queue.entry(job_id).or_default().push_back(prompt);
            }
            return Ok(Json(serde_json::json!({
                "status": "queued",
                "job_id": job_id.to_string(),
            })));
        }

        return Err((
            StatusCode::NOT_IMPLEMENTED,
            "Follow-up prompts are not supported for worker-mode sandbox jobs".to_string(),
        ));
    }

    let slot = state.scheduler.as_ref().ok_or((
        StatusCode::NOT_IMPLEMENTED,
        "Agent job prompts require the scheduler to be configured".to_string(),
    ))?;
    let scheduler_guard = slot.read().await;
    if let Some(ref scheduler) = *scheduler_guard
        && scheduler.is_running(job_id).await
    {
        scheduler
            .send_message(job_id, content)
            .await
            .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
        return Ok(Json(serde_json::json!({
            "status": "sent",
            "job_id": job_id.to_string(),
        })));
    }

    Err((
        StatusCode::NOT_FOUND,
        "Job not found or not running".to_string(),
    ))
}

pub async fn jobs_events_handler(
    State(state): State<Arc<GatewayState>>,
    Path(id): Path<String>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    let store = state.store.as_ref().ok_or((
        StatusCode::NOT_IMPLEMENTED,
        "Database not available".to_string(),
    ))?;

    let job_id: uuid::Uuid = id
        .parse()
        .map_err(|_| (StatusCode::BAD_REQUEST, "Invalid job ID".to_string()))?;

    let events = store
        .list_job_events(job_id, None)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    let events_json: Vec<serde_json::Value> = events
        .into_iter()
        .map(|e| {
            serde_json::json!({
                "id": e.id,
                "event_type": e.event_type,
                "data": e.data,
                "created_at": e.created_at.to_rfc3339(),
            })
        })
        .collect();

    Ok(Json(serde_json::json!({
        "job_id": job_id.to_string(),
        "events": events_json,
    })))
}
