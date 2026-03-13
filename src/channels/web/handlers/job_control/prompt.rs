//! Prompt-submission handlers for running jobs.

use std::sync::Arc;

use axum::{
    Json,
    extract::{Path, State},
    http::StatusCode,
};
use serde::Deserialize;

use crate::channels::web::server::GatewayState;

const MAX_PENDING_PROMPTS_PER_JOB: usize = 100;

#[derive(Deserialize)]
pub(super) struct JobPromptRequest {
    content: String,
    #[serde(default)]
    done: bool,
}

pub async fn jobs_prompt_handler(
    State(state): State<Arc<GatewayState>>,
    Path(id): Path<String>,
    Json(body): Json<JobPromptRequest>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    let store = state
        .store
        .as_ref()
        .ok_or_else(super::database_unavailable)?;
    let job_id: uuid::Uuid = id
        .parse()
        .map_err(|_| (StatusCode::BAD_REQUEST, "Invalid job ID".to_string()))?;

    let JobPromptRequest { content, done } = body;

    if let Some(job) = super::load_sandbox_job(store, job_id).await? {
        let mode = super::load_sandbox_job_mode(store, job_id).await?;
        if mode.as_deref() == Some("claude_code") {
            if !super::sandbox_job_accepts_prompts(&job.status) {
                return Err((
                    StatusCode::CONFLICT,
                    format!(
                        "Cannot queue prompts for sandbox job in state '{}'",
                        job.status
                    ),
                ));
            }
            let prompt_queue = state.prompt_queue.as_ref().ok_or((
                StatusCode::NOT_IMPLEMENTED,
                "Claude Code not configured".to_string(),
            ))?;
            let prompt = crate::orchestrator::api::PendingPrompt { content, done };
            {
                let mut queue = prompt_queue.lock().await;
                let pending = queue.entry(job_id).or_default();
                if pending.len() >= MAX_PENDING_PROMPTS_PER_JOB {
                    return Err((
                        StatusCode::TOO_MANY_REQUESTS,
                        format!(
                            "Prompt queue is full for job {job_id}; try again after pending prompts are processed"
                        ),
                    ));
                }
                pending.push_back(prompt);
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
    let scheduler = {
        let scheduler_guard = slot.read().await;
        scheduler_guard.as_ref().cloned()
    };
    let Some(scheduler) = scheduler else {
        return Err((
            StatusCode::NOT_IMPLEMENTED,
            "Scheduler not configured".to_string(),
        ));
    };

    if scheduler.is_running(job_id).await {
        scheduler
            .send_message(job_id, content)
            .await
            .map_err(|e| super::internal_error("Failed to forward job prompt", e))?;
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
