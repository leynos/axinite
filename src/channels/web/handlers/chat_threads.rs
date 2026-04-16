//! Chat thread-listing and thread-creation handlers.

use std::cmp::Reverse;
use std::sync::Arc;

use crate::db::EnsureConversationParams;

use axum::{
    Json, Router,
    extract::State,
    http::StatusCode,
    routing::{get, post},
};

use crate::channels::web::server::GatewayState;
use crate::channels::web::types::{ThreadInfo, ThreadListResponse};

pub fn routes() -> Router<Arc<GatewayState>> {
    Router::new()
        .route("/api/chat/threads", get(chat_threads_handler))
        .route("/api/chat/thread/new", post(chat_new_thread_handler))
}

pub async fn chat_threads_handler(
    State(state): State<Arc<GatewayState>>,
) -> Result<Json<ThreadListResponse>, (StatusCode, String)> {
    let session_manager = state.session_manager.as_ref().ok_or((
        StatusCode::SERVICE_UNAVAILABLE,
        "Session manager not available".to_string(),
    ))?;

    let session = session_manager.get_or_create_session(&state.user_id).await;
    let sess = session.lock().await;

    if let Some(ref store) = state.store {
        let assistant_id = store
            .get_or_create_assistant_conversation(&state.user_id, "gateway")
            .await
            .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

        if let Ok(summaries) = store
            .list_conversations_all_channels(&state.user_id, 50)
            .await
        {
            let mut assistant_thread = None;
            let mut threads = Vec::new();

            for s in &summaries {
                let info = ThreadInfo {
                    id: s.id,
                    state: "Idle".to_string(),
                    turn_count: s.message_count.max(0) as usize,
                    created_at: s.started_at.to_rfc3339(),
                    updated_at: s.last_activity.to_rfc3339(),
                    title: s.title.clone(),
                    thread_type: s.thread_type.clone(),
                    channel: Some(s.channel.clone()),
                };

                if s.id == assistant_id {
                    assistant_thread = Some(info);
                } else {
                    threads.push(info);
                }
            }

            if assistant_thread.is_none() {
                assistant_thread = Some(ThreadInfo {
                    id: assistant_id,
                    state: "Idle".to_string(),
                    turn_count: 0,
                    created_at: chrono::Utc::now().to_rfc3339(),
                    updated_at: chrono::Utc::now().to_rfc3339(),
                    title: None,
                    thread_type: Some("assistant".to_string()),
                    channel: Some("gateway".to_string()),
                });
            }

            return Ok(Json(ThreadListResponse {
                assistant_thread,
                threads,
                active_thread: sess.active_thread,
            }));
        }
    }

    let mut sorted_threads: Vec<_> = sess.threads.values().collect();
    sorted_threads.sort_by_key(|thread| Reverse(thread.updated_at));
    let threads = sorted_threads
        .into_iter()
        .map(|thread| ThreadInfo {
            id: thread.id,
            state: format!("{:?}", thread.state),
            turn_count: thread.turns.len(),
            created_at: thread.created_at.to_rfc3339(),
            updated_at: thread.updated_at.to_rfc3339(),
            title: None,
            thread_type: None,
            channel: Some("gateway".to_string()),
        })
        .collect();

    Ok(Json(ThreadListResponse {
        assistant_thread: None,
        threads,
        active_thread: sess.active_thread,
    }))
}

pub async fn chat_new_thread_handler(
    State(state): State<Arc<GatewayState>>,
) -> Result<Json<ThreadInfo>, (StatusCode, String)> {
    let session_manager = state.session_manager.as_ref().ok_or((
        StatusCode::SERVICE_UNAVAILABLE,
        "Session manager not available".to_string(),
    ))?;

    let session = session_manager.get_or_create_session(&state.user_id).await;
    let (thread_id, info) = {
        let mut sess = session.lock().await;
        let thread = sess.create_thread();
        let id = thread.id;
        let info = ThreadInfo {
            id: thread.id,
            state: format!("{:?}", thread.state),
            turn_count: thread.turns.len(),
            created_at: thread.created_at.to_rfc3339(),
            updated_at: thread.updated_at.to_rfc3339(),
            title: None,
            thread_type: Some("thread".to_string()),
            channel: Some("gateway".to_string()),
        };
        (id, info)
    };

    if let Some(ref store) = state.store {
        if let Err(e) = store
            .ensure_conversation(EnsureConversationParams {
                id: thread_id,
                channel: "gateway",
                user_id: &state.user_id,
                thread_id: None,
            })
            .await
        {
            tracing::warn!("Failed to persist new thread: {}", e);
        }
        let metadata_val = serde_json::json!("thread");
        if let Err(e) = store
            .update_conversation_metadata_field(thread_id, "thread_type", &metadata_val)
            .await
        {
            tracing::warn!("Failed to set thread_type metadata: {}", e);
        }
    }

    Ok(Json(info))
}
