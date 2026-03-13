//! Chat history handlers for the web gateway.

use std::sync::Arc;

use axum::{
    Json, Router,
    extract::{Query, State},
    http::StatusCode,
    routing::get,
};
use serde::Deserialize;
use uuid::Uuid;

use crate::channels::web::handlers::chat_threads;
use crate::channels::web::server::GatewayState;
use crate::channels::web::types::{HistoryResponse, PendingApprovalInfo, ToolCallInfo, TurnInfo};
use crate::channels::web::util::{build_turns_from_db_messages, truncate_preview};

pub fn routes() -> Router<Arc<GatewayState>> {
    Router::new()
        .route("/api/chat/history", get(chat_history_handler))
        .merge(chat_threads::routes())
}

#[derive(Deserialize)]
pub struct HistoryQuery {
    pub thread_id: Option<String>,
    pub limit: Option<usize>,
    pub before: Option<String>,
}

pub async fn chat_history_handler(
    State(state): State<Arc<GatewayState>>,
    Query(query): Query<HistoryQuery>,
) -> Result<Json<HistoryResponse>, (StatusCode, String)> {
    let session_manager = state.session_manager.as_ref().ok_or((
        StatusCode::SERVICE_UNAVAILABLE,
        "Session manager not available".to_string(),
    ))?;

    let session = session_manager.get_or_create_session(&state.user_id).await;
    let sess = session.lock().await;

    let limit = query.limit.unwrap_or(50);
    let before_cursor = query
        .before
        .as_deref()
        .map(|value| {
            chrono::DateTime::parse_from_rfc3339(value)
                .map(|dt| dt.with_timezone(&chrono::Utc))
                .map_err(|_| {
                    (
                        StatusCode::BAD_REQUEST,
                        "Invalid 'before' timestamp".to_string(),
                    )
                })
        })
        .transpose()?;

    let thread_id = if let Some(ref tid) = query.thread_id {
        Uuid::parse_str(tid)
            .map_err(|_| (StatusCode::BAD_REQUEST, "Invalid thread_id".to_string()))?
    } else {
        sess.active_thread
            .ok_or((StatusCode::NOT_FOUND, "No active thread".to_string()))?
    };

    if query.thread_id.is_some()
        && let Some(ref store) = state.store
    {
        let owned = store
            .conversation_belongs_to_user(thread_id, &state.user_id)
            .await
            .unwrap_or(false);
        if !owned && !sess.threads.contains_key(&thread_id) {
            return Err((StatusCode::NOT_FOUND, "Thread not found".to_string()));
        }
    }

    if before_cursor.is_some()
        && let Some(ref store) = state.store
    {
        let (messages, has_more) = store
            .list_conversation_messages_paginated(thread_id, before_cursor, limit as i64)
            .await
            .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

        let oldest_timestamp = messages.first().map(|m| m.created_at.to_rfc3339());
        let turns = build_turns_from_db_messages(&messages);
        return Ok(Json(HistoryResponse {
            thread_id,
            turns,
            has_more,
            oldest_timestamp,
            pending_approval: None,
        }));
    }

    if let Some(thread) = sess.threads.get(&thread_id)
        && (!thread.turns.is_empty() || thread.pending_approval.is_some())
    {
        let turns = thread
            .turns
            .iter()
            .map(|turn| TurnInfo {
                turn_number: turn.turn_number,
                user_input: turn.user_input.clone(),
                response: turn.response.clone(),
                state: format!("{:?}", turn.state),
                started_at: turn.started_at.to_rfc3339(),
                completed_at: turn.completed_at.map(|dt| dt.to_rfc3339()),
                tool_calls: turn
                    .tool_calls
                    .iter()
                    .map(|tool_call| ToolCallInfo {
                        name: tool_call.name.clone(),
                        has_result: tool_call.result.is_some(),
                        has_error: tool_call.error.is_some(),
                        result_preview: tool_call.result.as_ref().map(|result| {
                            let string = match result {
                                serde_json::Value::String(s) => s.clone(),
                                other => other.to_string(),
                            };
                            truncate_preview(&string, 500)
                        }),
                        error: tool_call.error.clone(),
                    })
                    .collect(),
            })
            .collect();

        let pending_approval =
            thread
                .pending_approval
                .as_ref()
                .map(|approval| PendingApprovalInfo {
                    request_id: approval.request_id.to_string(),
                    tool_name: approval.tool_name.clone(),
                    description: approval.description.clone(),
                    parameters: serde_json::to_string_pretty(&approval.parameters)
                        .unwrap_or_default(),
                });

        return Ok(Json(HistoryResponse {
            thread_id,
            turns,
            has_more: false,
            oldest_timestamp: None,
            pending_approval,
        }));
    }

    if let Some(ref store) = state.store {
        let (messages, has_more) = store
            .list_conversation_messages_paginated(thread_id, None, limit as i64)
            .await
            .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

        if !messages.is_empty() {
            let oldest_timestamp = messages.first().map(|m| m.created_at.to_rfc3339());
            let turns = build_turns_from_db_messages(&messages);
            return Ok(Json(HistoryResponse {
                thread_id,
                turns,
                has_more,
                oldest_timestamp,
                pending_approval: None,
            }));
        }
    }

    Ok(Json(HistoryResponse {
        thread_id,
        turns: Vec::new(),
        has_more: false,
        oldest_timestamp: None,
        pending_approval: None,
    }))
}
