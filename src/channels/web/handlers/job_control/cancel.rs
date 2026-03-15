//! Cancellation helpers for web job-control handlers.

use std::sync::Arc;

use axum::http::StatusCode;
use uuid::Uuid;

use crate::channels::web::server::GatewayState;
use crate::db::Database;

use super::internal_error;

pub(super) async fn cancel_sandbox_job(
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

pub(super) async fn cancel_agent_job(
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
