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
use crate::db::Database;

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

const DEFAULT_HISTORY_LIMIT: usize = 50;
const MAX_HISTORY_LIMIT: usize = 200;

/// A pagination cursor: the timestamp and id of the oldest message already seen.
type BeforeCursor = (chrono::DateTime<chrono::Utc>, Uuid);

fn parse_before_cursor(value: &str) -> Result<BeforeCursor, ()> {
    let Some((timestamp, message_id)) = value.split_once('|') else {
        return parse_timestamp_only_cursor(value);
    };
    parse_timestamped_cursor(timestamp, message_id)
}

/// Parse a `timestamp|message-id` history cursor.
fn parse_timestamped_cursor(
    timestamp: &str,
    message_id: &str,
) -> Result<(chrono::DateTime<chrono::Utc>, Uuid), ()> {
    let parsed_timestamp = parse_cursor_timestamp(timestamp)?;
    let parsed_id = parse_cursor_message_id(message_id)?;
    Ok((parsed_timestamp, parsed_id))
}

/// Parse a legacy history cursor that carries only a timestamp.
fn parse_timestamp_only_cursor(value: &str) -> Result<(chrono::DateTime<chrono::Utc>, Uuid), ()> {
    Ok((parse_cursor_timestamp(value)?, Uuid::nil()))
}

/// Parse an RFC 3339 history cursor timestamp as UTC.
fn parse_cursor_timestamp(value: &str) -> Result<chrono::DateTime<chrono::Utc>, ()> {
    chrono::DateTime::parse_from_rfc3339(value)
        .map(|dt| dt.with_timezone(&chrono::Utc))
        .map_err(|_| ())
}

/// Parse the UUID half of a `timestamp|message-id` history cursor.
fn parse_cursor_message_id(value: &str) -> Result<Uuid, ()> {
    Uuid::parse_str(value).map_err(|_| ())
}

async fn load_stored_history(
    store: &Arc<dyn Database>,
    thread_id: Uuid,
    before_cursor: Option<(chrono::DateTime<chrono::Utc>, Uuid)>,
    limit: usize,
) -> Result<HistoryResponse, (StatusCode, String)> {
    let (messages, has_more) = store
        .list_conversation_messages_paginated(thread_id, before_cursor, limit)
        .await
        .map_err(|e| match e {
            crate::error::DatabaseError::Validation(msg) => (StatusCode::BAD_REQUEST, msg),
            _ => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
        })?;
    let oldest_timestamp = messages
        .first()
        .map(|message| format!("{}|{}", message.created_at.to_rfc3339(), message.id));
    let turns = build_turns_from_db_messages(&messages);

    Ok(HistoryResponse {
        thread_id,
        turns,
        has_more,
        oldest_timestamp,
        pending_approval: None,
    })
}

/// Return `true` when an in-memory thread has renderable content.
fn thread_has_content(thread: &crate::agent::session::Thread) -> bool {
    !thread.turns.is_empty() || thread.pending_approval.is_some()
}

/// Reject a requested thread that neither the user's stored conversations nor
/// the live session knows about.
async fn verify_thread_known(
    state: &GatewayState,
    thread_id: Uuid,
    in_session: bool,
) -> Result<(), (StatusCode, String)> {
    let Some(ref store) = state.store else {
        return Ok(());
    };
    let owned = store
        .conversation_belongs_to_user(thread_id, &state.user_id)
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Failed to verify thread ownership: {e}"),
            )
        })?;
    if !owned && !in_session {
        return Err((StatusCode::NOT_FOUND, "Thread not found".to_string()));
    }
    Ok(())
}

/// Summarize one tool call for the history response.
fn tool_call_info(tool_call: &crate::agent::session::TurnToolCall) -> ToolCallInfo {
    ToolCallInfo {
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
    }
}

/// Summarize one in-memory turn for the history response.
fn turn_info(turn: &crate::agent::session::Turn) -> TurnInfo {
    TurnInfo {
        turn_number: turn.turn_number,
        user_input: turn.user_input.clone(),
        response: turn.response.clone(),
        state: format!("{:?}", turn.state),
        started_at: turn.started_at.to_rfc3339(),
        completed_at: turn.completed_at.map(|dt| dt.to_rfc3339()),
        tool_calls: turn.tool_calls.iter().map(tool_call_info).collect(),
    }
}

/// Build the history response from a live in-memory thread.
fn in_memory_history(
    thread: &crate::agent::session::Thread,
    thread_id: Uuid,
) -> Result<HistoryResponse, (StatusCode, String)> {
    let pending_approval = thread
        .pending_approval
        .as_ref()
        .map(|approval| {
            serde_json::to_string_pretty(&approval.parameters).map(|parameters| {
                PendingApprovalInfo {
                    request_id: approval.request_id.to_string(),
                    tool_name: approval.tool_name.clone(),
                    description: approval.description.clone(),
                    parameters,
                }
            })
        })
        .transpose()
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Failed to serialize pending approval parameters: {e}"),
            )
        })?;

    Ok(HistoryResponse {
        thread_id,
        turns: thread.turns.iter().map(turn_info).collect(),
        has_more: false,
        oldest_timestamp: None,
        pending_approval,
    })
}

/// Parse the optional `before` pagination cursor, rejecting a malformed one.
fn parse_optional_before(
    before: Option<&str>,
) -> Result<Option<BeforeCursor>, (StatusCode, String)> {
    before.map(parse_before_cursor).transpose().map_err(|_| {
        (
            StatusCode::BAD_REQUEST,
            "Invalid 'before' cursor".to_string(),
        )
    })
}

/// The thread to report: the one named in the query, else the session's active thread.
async fn requested_thread_id(
    query: &HistoryQuery,
    session: &Arc<tokio::sync::Mutex<crate::agent::session::Session>>,
) -> Result<Uuid, (StatusCode, String)> {
    if let Some(ref tid) = query.thread_id {
        return Uuid::parse_str(tid)
            .map_err(|_| (StatusCode::BAD_REQUEST, "Invalid thread_id".to_string()));
    }
    let sess = session.lock().await;
    sess.active_thread
        .ok_or((StatusCode::NOT_FOUND, "No active thread".to_string()))
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

    // Early validation: reject limit == 0 before any history-branch logic
    if query.limit == Some(0) {
        return Err((
            StatusCode::BAD_REQUEST,
            "conversation message pagination limit must be > 0".to_string(),
        ));
    }

    let limit = query
        .limit
        .unwrap_or(DEFAULT_HISTORY_LIMIT)
        .clamp(1, MAX_HISTORY_LIMIT);
    let before_cursor = parse_optional_before(query.before.as_deref())?;

    let requested_thread_id = requested_thread_id(&query, &session).await?;

    let session_thread = {
        let sess = session.lock().await;
        sess.threads.get(&requested_thread_id).cloned()
    };

    if query.thread_id.is_some() {
        verify_thread_known(&state, requested_thread_id, session_thread.is_some()).await?;
    }

    if before_cursor.is_some()
        && let Some(ref store) = state.store
    {
        return load_stored_history(store, requested_thread_id, before_cursor, limit)
            .await
            .map(Json);
    }

    if let Some(thread) = session_thread.as_ref()
        && thread_has_content(thread)
    {
        return in_memory_history(thread, requested_thread_id).map(Json);
    }

    if let Some(ref store) = state.store {
        let history = load_stored_history(store, requested_thread_id, None, limit).await?;
        if !history.turns.is_empty() {
            return Ok(Json(history));
        }
    }

    Ok(Json(HistoryResponse {
        thread_id: requested_thread_id,
        turns: Vec::new(),
        has_more: false,
        oldest_timestamp: None,
        pending_approval: None,
    }))
}
