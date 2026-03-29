//! Job summary, listing, and detail handlers for the web gateway.

use std::collections::HashSet;
use std::sync::Arc;

use axum::{
    Json, Router,
    extract::{Path, State},
    http::StatusCode,
    routing::get,
};
use uuid::Uuid;

use crate::channels::web::handlers::{job_control, job_files};
use crate::channels::web::server::GatewayState;
use crate::channels::web::types::*;

pub fn routes() -> Router<Arc<GatewayState>> {
    Router::new()
        .route("/api/jobs", get(jobs_list_handler))
        .route("/api/jobs/summary", get(jobs_summary_handler))
        .route("/api/jobs/{id}", get(jobs_detail_handler))
        .merge(job_control::routes())
        .merge(job_files::routes())
}

pub async fn jobs_list_handler(
    State(state): State<Arc<GatewayState>>,
) -> Result<Json<JobListResponse>, (StatusCode, String)> {
    let store = state.store.as_ref().ok_or((
        StatusCode::SERVICE_UNAVAILABLE,
        "Database not available".to_string(),
    ))?;

    let mut jobs: Vec<JobInfo> = Vec::new();
    let mut seen_ids: HashSet<Uuid> = HashSet::new();

    match store.list_sandbox_jobs().await {
        Ok(sandbox_jobs) => {
            for j in &sandbox_jobs {
                let ui_state = match j.status.as_str() {
                    "creating" => "pending",
                    "running" => "in_progress",
                    s => s,
                };
                seen_ids.insert(j.id);
                jobs.push(JobInfo {
                    id: j.id,
                    title: j.task.clone(),
                    state: ui_state.to_string(),
                    user_id: j.user_id.clone(),
                    created_at: j.created_at.to_rfc3339(),
                    started_at: j.started_at.map(|dt| dt.to_rfc3339()),
                });
            }
        }
        Err(e) => {
            tracing::warn!("Failed to list sandbox jobs: {e}");
        }
    }

    match store.list_agent_jobs().await {
        Ok(agent_jobs) => {
            for j in &agent_jobs {
                if seen_ids.contains(&j.id) {
                    continue;
                }
                jobs.push(JobInfo {
                    id: j.id,
                    title: j.title.clone(),
                    state: j.status.clone(),
                    user_id: j.user_id.clone(),
                    created_at: j.created_at.to_rfc3339(),
                    started_at: j.started_at.map(|dt| dt.to_rfc3339()),
                });
            }
        }
        Err(e) => {
            tracing::warn!("Failed to list agent jobs: {e}");
        }
    }

    jobs.sort_by(|a, b| b.created_at.cmp(&a.created_at));

    Ok(Json(JobListResponse { jobs }))
}

pub async fn jobs_summary_handler(
    State(state): State<Arc<GatewayState>>,
) -> Result<Json<JobSummaryResponse>, (StatusCode, String)> {
    let store = state.store.as_ref().ok_or((
        StatusCode::SERVICE_UNAVAILABLE,
        "Database not available".to_string(),
    ))?;

    let mut total = 0;
    let mut pending = 0;
    let mut in_progress = 0;
    let mut completed = 0;
    let mut failed = 0;
    let mut stuck = 0;

    match store.sandbox_job_summary().await {
        Ok(s) => {
            total += s.total;
            pending += s.creating;
            in_progress += s.running;
            completed += s.completed;
            failed += s.failed + s.interrupted;
        }
        Err(e) => {
            tracing::warn!("Failed to fetch sandbox job summary: {e}");
        }
    }

    match store.agent_job_summary().await {
        Ok(s) => {
            total += s.total;
            pending += s.pending;
            in_progress += s.in_progress;
            completed += s.completed;
            failed += s.failed;
            stuck += s.stuck;
        }
        Err(e) => {
            tracing::warn!("Failed to fetch agent job summary: {e}");
        }
    }

    Ok(Json(JobSummaryResponse {
        total,
        pending,
        in_progress,
        completed,
        failed,
        stuck,
    }))
}

pub async fn jobs_detail_handler(
    State(state): State<Arc<GatewayState>>,
    Path(id): Path<String>,
) -> Result<Json<JobDetailResponse>, (StatusCode, String)> {
    let store = state.store.as_ref().ok_or((
        StatusCode::SERVICE_UNAVAILABLE,
        "Database not available".to_string(),
    ))?;

    let job_id = Uuid::parse_str(&id)
        .map_err(|_| (StatusCode::BAD_REQUEST, "Invalid job ID".to_string()))?;

    if let Ok(Some(job)) = store.get_sandbox_job(job_id).await {
        let browse_id = std::path::Path::new(&job.project_dir)
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| job.id.to_string());

        let ui_state = match job.status.as_str() {
            "creating" => "pending",
            "running" => "in_progress",
            s => s,
        };

        let elapsed_secs = job.started_at.map(|start| {
            let end = job.completed_at.unwrap_or_else(chrono::Utc::now);
            (end - start).num_seconds().max(0) as u64
        });

        let mut transitions = Vec::new();
        if let Some(started) = job.started_at {
            transitions.push(TransitionInfo {
                from: "creating".to_string(),
                to: "running".to_string(),
                timestamp: started.to_rfc3339(),
                reason: None,
            });
        }
        if let Some(completed) = job.completed_at {
            transitions.push(TransitionInfo {
                from: "running".to_string(),
                to: job.status.clone(),
                timestamp: completed.to_rfc3339(),
                reason: job.failure_reason.clone(),
            });
        }

        let mode = store.get_sandbox_job_mode(job.id).await.ok().flatten();
        let is_claude_code = mode == Some(crate::db::SandboxMode::ClaudeCode);

        return Ok(Json(JobDetailResponse {
            id: job.id,
            title: job.task.clone(),
            description: String::new(),
            state: ui_state.to_string(),
            user_id: job.user_id.clone(),
            created_at: job.created_at.to_rfc3339(),
            started_at: job.started_at.map(|dt| dt.to_rfc3339()),
            completed_at: job.completed_at.map(|dt| dt.to_rfc3339()),
            elapsed_secs,
            project_dir: Some(job.project_dir.clone()),
            browse_url: Some(format!("/projects/{browse_id}/")),
            job_mode: mode
                .filter(|mode| *mode != crate::db::SandboxMode::Worker)
                .map(|mode| mode.to_string()),
            transitions,
            can_restart: state.job_manager.is_some(),
            can_prompt: is_claude_code && state.prompt_queue.is_some(),
            job_kind: Some("sandbox".to_string()),
        }));
    }

    if let Ok(Some(ctx)) = store.get_job(job_id).await {
        let elapsed_secs = ctx.started_at.map(|start| {
            let end = ctx.completed_at.unwrap_or_else(chrono::Utc::now);
            (end - start).num_seconds().max(0) as u64
        });

        let is_promptable = matches!(
            ctx.state,
            crate::context::JobState::Pending | crate::context::JobState::InProgress
        );
        return Ok(Json(JobDetailResponse {
            id: ctx.job_id,
            title: ctx.title.clone(),
            description: ctx.description.clone(),
            state: ctx.state.to_string(),
            user_id: ctx.user_id.clone(),
            created_at: ctx.created_at.to_rfc3339(),
            started_at: ctx.started_at.map(|dt| dt.to_rfc3339()),
            completed_at: ctx.completed_at.map(|dt| dt.to_rfc3339()),
            elapsed_secs,
            project_dir: None,
            browse_url: None,
            job_mode: None,
            transitions: Vec::new(),
            can_restart: state.scheduler.is_some(),
            can_prompt: is_promptable && state.scheduler.is_some(),
            job_kind: Some("agent".to_string()),
        }));
    }

    Err((StatusCode::NOT_FOUND, "Job not found".to_string()))
}
