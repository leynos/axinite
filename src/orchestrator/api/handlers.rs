use std::sync::Arc;

use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use uuid::Uuid;

use super::OrchestratorState;
use crate::channels::web::types::SseEvent;
use crate::context::JobContext;
use crate::llm::{CompletionRequest, ToolCompletionRequest};
use crate::tools::builtin::extension_tools::ExtensionToolKind;
use crate::worker::api::{
    CompletionReport, CredentialResponse, JobDescription, JobEventPayload, ProxyCompletionRequest,
    ProxyCompletionResponse, ProxyExtensionToolRequest, ProxyExtensionToolResponse,
    ProxyToolCompletionRequest, ProxyToolCompletionResponse, StatusUpdate,
};

// All /worker/ handlers below are behind the worker_auth_middleware route_layer,
// so they don't need to validate tokens themselves.

pub(super) async fn health_check() -> &'static str {
    "ok"
}

pub(super) async fn get_job(
    State(state): State<OrchestratorState>,
    Path(job_id): Path<Uuid>,
) -> Result<Json<JobDescription>, StatusCode> {
    let handle = state
        .job_manager
        .get_handle(job_id)
        .await
        .ok_or(StatusCode::NOT_FOUND)?;

    Ok(Json(JobDescription {
        title: format!("Job {}", job_id),
        description: handle.task_description,
        project_dir: handle.project_dir.map(|p| p.display().to_string()),
    }))
}

pub(super) async fn llm_complete(
    State(state): State<OrchestratorState>,
    Path(job_id): Path<Uuid>,
    Json(req): Json<ProxyCompletionRequest>,
) -> Result<Json<ProxyCompletionResponse>, StatusCode> {
    let completion_req = CompletionRequest {
        messages: req.messages,
        model: req.model,
        max_tokens: req.max_tokens,
        temperature: req.temperature,
        stop_sequences: req.stop_sequences,
        metadata: std::collections::HashMap::new(),
    };

    let resp = state.llm.complete(completion_req).await.map_err(|e| {
        tracing::error!("LLM completion failed for job {}: {}", job_id, e);
        StatusCode::BAD_GATEWAY
    })?;

    Ok(Json(ProxyCompletionResponse {
        content: resp.content,
        input_tokens: resp.input_tokens,
        output_tokens: resp.output_tokens,
        finish_reason: format_finish_reason(resp.finish_reason),
        cache_read_input_tokens: resp.cache_read_input_tokens,
        cache_creation_input_tokens: resp.cache_creation_input_tokens,
    }))
}

pub(super) async fn llm_complete_with_tools(
    State(state): State<OrchestratorState>,
    Path(job_id): Path<Uuid>,
    Json(req): Json<ProxyToolCompletionRequest>,
) -> Result<Json<ProxyToolCompletionResponse>, StatusCode> {
    let tool_req = ToolCompletionRequest {
        messages: req.messages,
        tools: req.tools,
        model: req.model,
        max_tokens: req.max_tokens,
        temperature: req.temperature,
        tool_choice: req.tool_choice,
        metadata: std::collections::HashMap::new(),
    };

    let resp = state.llm.complete_with_tools(tool_req).await.map_err(|e| {
        tracing::error!("LLM tool completion failed for job {}: {}", job_id, e);
        StatusCode::BAD_GATEWAY
    })?;

    Ok(Json(ProxyToolCompletionResponse {
        content: resp.content,
        tool_calls: resp.tool_calls,
        input_tokens: resp.input_tokens,
        output_tokens: resp.output_tokens,
        finish_reason: format_finish_reason(resp.finish_reason),
        cache_read_input_tokens: resp.cache_read_input_tokens,
        cache_creation_input_tokens: resp.cache_creation_input_tokens,
    }))
}

pub(super) async fn execute_extension_tool(
    State(state): State<OrchestratorState>,
    Path(job_id): Path<Uuid>,
    Json(req): Json<ProxyExtensionToolRequest>,
) -> Result<Json<ProxyExtensionToolResponse>, StatusCode> {
    let Some(kind) = ExtensionToolKind::ALL
        .iter()
        .find(|kind| kind.name() == req.tool_name)
        .copied()
    else {
        tracing::warn!(
            job_id = %job_id,
            tool = %req.tool_name,
            "Worker attempted non-extension tool proxy execution"
        );
        return Err(StatusCode::BAD_REQUEST);
    };

    if !ExtensionToolKind::HOSTED_WORKER_PROXY_SAFE.contains(&kind) {
        tracing::warn!(
            job_id = %job_id,
            tool = %kind.name(),
            "Worker attempted restricted extension proxy execution"
        );
        return Err(StatusCode::FORBIDDEN);
    }

    let tool = state
        .tools
        .get(&req.tool_name)
        .await
        .ok_or(StatusCode::NOT_FOUND)?;

    if tool.requires_approval(&req.params).is_required() {
        tracing::warn!(
            job_id = %job_id,
            tool = %kind.name(),
            "Worker attempted approval-gated extension proxy execution"
        );
        return Err(StatusCode::FORBIDDEN);
    }

    let mut ctx = JobContext::with_user(
        state.user_id.clone(),
        "Hosted extension tool",
        format!("Hosted execution of {}", req.tool_name),
    );
    ctx.job_id = job_id;
    let output = tool.execute(req.params, &ctx).await.map_err(|e| {
        tracing::warn!(
            job_id = %job_id,
            tool = %tool.name(),
            error = %e,
            "Extension tool proxy execution failed"
        );
        StatusCode::BAD_GATEWAY
    })?;

    Ok(Json(ProxyExtensionToolResponse { output }))
}

pub(super) async fn report_status(
    State(state): State<OrchestratorState>,
    Path(job_id): Path<Uuid>,
    Json(update): Json<StatusUpdate>,
) -> Result<StatusCode, StatusCode> {
    tracing::debug!(
        job_id = %job_id,
        state = %update.state,
        iteration = update.iteration,
        "Worker status update"
    );

    state
        .job_manager
        .update_worker_status(job_id, update.message, update.iteration)
        .await;

    Ok(StatusCode::OK)
}

pub(super) async fn report_complete(
    State(state): State<OrchestratorState>,
    Path(job_id): Path<Uuid>,
    Json(report): Json<CompletionReport>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    if report.success {
        tracing::info!(job_id = %job_id, "Worker reported job complete");
    } else {
        tracing::warn!(
            job_id = %job_id,
            message = ?report.message,
            "Worker reported job failure"
        );
    }

    let result = crate::orchestrator::job_manager::CompletionResult {
        success: report.success,
        message: report.message.clone(),
    };
    if let Err(e) = state.job_manager.complete_job(job_id, result).await {
        tracing::error!(job_id = %job_id, "Failed to complete job cleanup: {}", e);
    }

    Ok(Json(serde_json::json!({"status": "ok"})))
}

/// Receive a job event from a worker or Claude Code bridge and broadcast + persist it.
pub(super) async fn job_event_handler(
    State(state): State<OrchestratorState>,
    Path(job_id): Path<Uuid>,
    Json(payload): Json<JobEventPayload>,
) -> Result<StatusCode, StatusCode> {
    tracing::debug!(
        job_id = %job_id,
        event_type = %payload.event_type,
        "Job event received"
    );

    if let Some(ref store) = state.store {
        let store = Arc::clone(store);
        let event_type = payload.event_type.clone();
        let data = payload.data.clone();
        tokio::spawn(async move {
            if let Err(e) = store.save_job_event(job_id, &event_type, &data).await {
                tracing::warn!(job_id = %job_id, "Failed to persist job event: {}", e);
            }
        });
    }

    let job_id_str = job_id.to_string();
    let sse_event = match payload.event_type.as_str() {
        "message" => SseEvent::JobMessage {
            job_id: job_id_str,
            role: payload
                .data
                .get("role")
                .and_then(|v| v.as_str())
                .unwrap_or("assistant")
                .to_string(),
            content: payload
                .data
                .get("content")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string(),
        },
        "tool_use" => SseEvent::JobToolUse {
            job_id: job_id_str,
            tool_name: payload
                .data
                .get("tool_name")
                .and_then(|v| v.as_str())
                .unwrap_or("unknown")
                .to_string(),
            input: payload
                .data
                .get("input")
                .cloned()
                .unwrap_or(serde_json::Value::Null),
        },
        "tool_result" => SseEvent::JobToolResult {
            job_id: job_id_str,
            tool_name: payload
                .data
                .get("tool_name")
                .and_then(|v| v.as_str())
                .unwrap_or("unknown")
                .to_string(),
            output: payload
                .data
                .get("output")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string(),
        },
        "result" => SseEvent::JobResult {
            job_id: job_id_str,
            status: payload
                .data
                .get("status")
                .and_then(|v| v.as_str())
                .unwrap_or("unknown")
                .to_string(),
            session_id: payload
                .data
                .get("session_id")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string()),
        },
        _ => SseEvent::JobStatus {
            job_id: job_id_str,
            message: payload
                .data
                .get("message")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string(),
        },
    };

    if let Some(ref tx) = state.job_event_tx {
        let _ = tx.send((job_id, sse_event));
    }

    Ok(StatusCode::OK)
}

/// Return the next queued follow-up prompt for a Claude Code bridge.
/// Returns 204 No Content if no prompt is available.
pub(super) async fn get_prompt_handler(
    State(state): State<OrchestratorState>,
    Path(job_id): Path<Uuid>,
) -> Result<(StatusCode, Json<serde_json::Value>), StatusCode> {
    let mut queue = state.prompt_queue.lock().await;
    if let Some(prompts) = queue.get_mut(&job_id)
        && let Some(prompt) = prompts.pop_front()
    {
        return Ok((
            StatusCode::OK,
            Json(serde_json::json!({
                "content": prompt.content,
                "done": prompt.done,
            })),
        ));
    }

    Ok((StatusCode::NO_CONTENT, Json(serde_json::Value::Null)))
}

/// Serve decrypted credentials for a job's granted secrets.
///
/// Returns 204 if no grants exist, 503 if no secrets store is configured,
/// or a JSON array of `{ env_var, value }` pairs.
pub(super) async fn get_credentials_handler(
    State(state): State<OrchestratorState>,
    Path(job_id): Path<Uuid>,
) -> Result<(StatusCode, Json<serde_json::Value>), StatusCode> {
    let grants = match state.token_store.get_grants(job_id).await {
        Some(g) if !g.is_empty() => g,
        _ => return Ok((StatusCode::NO_CONTENT, Json(serde_json::Value::Null))),
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

        if let Ok(secret) = secrets.get(&state.user_id, &grant.secret_name).await
            && let Err(e) = secrets.record_usage(secret.id).await
        {
            tracing::warn!(
                job_id = %job_id,
                "Failed to record credential usage: {}", e
            );
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

    Ok((
        StatusCode::OK,
        Json(serde_json::to_value(&credentials).unwrap_or(serde_json::Value::Null)),
    ))
}

fn format_finish_reason(reason: crate::llm::FinishReason) -> String {
    match reason {
        crate::llm::FinishReason::Stop => "stop".to_string(),
        crate::llm::FinishReason::Length => "length".to_string(),
        crate::llm::FinishReason::ToolUse => "tool_use".to_string(),
        crate::llm::FinishReason::ContentFilter => "content_filter".to_string(),
        crate::llm::FinishReason::Unknown => "unknown".to_string(),
    }
}
