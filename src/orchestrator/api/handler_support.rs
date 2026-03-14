//! Prompt and credential helpers for the orchestrator worker API.

use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use uuid::Uuid;

use super::OrchestratorState;
use crate::worker::api::CredentialResponse;

/// Return the next queued follow-up prompt for a Claude Code bridge.
pub(super) async fn get_prompt_handler(
    State(state): State<OrchestratorState>,
    Path(job_id): Path<Uuid>,
) -> Result<Response, StatusCode> {
    let mut queue = state.prompt_queue.lock().await;
    if let Some(prompts) = queue.get_mut(&job_id)
        && let Some(prompt) = prompts.pop_front()
    {
        let should_prune = prompts.is_empty();
        if should_prune {
            queue.remove(&job_id);
        }
        return Ok((
            StatusCode::OK,
            Json(serde_json::json!({
                "content": prompt.content,
                "done": prompt.done,
            })),
        )
            .into_response());
    }

    Ok(StatusCode::NO_CONTENT.into_response())
}

/// Serve decrypted credentials for a job's granted secrets.
pub(super) async fn get_credentials_handler(
    State(state): State<OrchestratorState>,
    Path(job_id): Path<Uuid>,
) -> Result<Response, StatusCode> {
    let grants = match state.token_store.get_grants(job_id).await {
        Some(g) if !g.is_empty() => g,
        _ => return Ok(StatusCode::NO_CONTENT.into_response()),
    };

    let secrets = state.secrets_store.as_ref().ok_or_else(|| {
        tracing::error!("Credentials requested but no secrets store configured");
        StatusCode::SERVICE_UNAVAILABLE
    })?;

    let mut credentials: Vec<CredentialResponse> = Vec::with_capacity(grants.len());

    for grant in &grants {
        let decrypted = secrets
            .get_decrypted(&state.user_id, &grant.secret_name)
            .await
            .map_err(|e| {
                tracing::error!(
                    job_id = %job_id,
                    "Failed to decrypt secret for credential grant: {}", e
                );
                StatusCode::INTERNAL_SERVER_ERROR
            })?;

        match secrets.get(&state.user_id, &grant.secret_name).await {
            Ok(secret) => {
                if let Err(e) = secrets.record_usage(secret.id).await {
                    tracing::warn!(
                        job_id = %job_id,
                        "Failed to record credential usage: {}", e
                    );
                }
            }
            Err(e) => {
                tracing::warn!(
                    job_id = %job_id,
                    "Failed to read credential metadata before usage recording: {}", e
                );
            }
        }

        tracing::debug!(
            job_id = %job_id,
            env_var = %grant.env_var,
            "Serving credential to container"
        );

        credentials.push(CredentialResponse {
            env_var: grant.env_var.clone(),
            value: decrypted.expose().to_string(),
        });
    }

    let body = serde_json::to_value(&credentials).map_err(|e| {
        tracing::error!(
            job_id = %job_id,
            "Failed to serialize credential response payload: {}", e
        );
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    Ok((StatusCode::OK, Json(body)).into_response())
}

pub(super) fn format_finish_reason(reason: crate::llm::FinishReason) -> String {
    match reason {
        crate::llm::FinishReason::Stop => "stop".to_string(),
        crate::llm::FinishReason::Length => "length".to_string(),
        crate::llm::FinishReason::ToolUse => "tool_use".to_string(),
        crate::llm::FinishReason::ContentFilter => "content_filter".to_string(),
        crate::llm::FinishReason::Unknown => "unknown".to_string(),
    }
}
