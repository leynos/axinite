//! LibSQL singleton-conversation helpers for routine, heartbeat, and assistant
//! threads. Routine and heartbeat helpers open an immediate transaction so the
//! lookup and optional insert happen atomically on one connection, while the
//! assistant helper relies on insert-or-ignore plus a follow-up read.

use super::*;

pub(super) async fn get_or_create_routine_conversation(
    backend: &LibSqlBackend,
    routine_id: Uuid,
    routine_name: &str,
    user_id: &str,
) -> Result<Uuid, DatabaseError> {
    let conn = backend.connect().await?;
    let rid = routine_id.to_string();

    conn.execute("BEGIN IMMEDIATE", params![])
        .await
        .map_err(|e| DatabaseError::Query(e.to_string()))?;

    let result: Result<Uuid, DatabaseError> = async {
            let mut rows = conn
                .query(
                    r#"
                    SELECT id FROM conversations
                    WHERE user_id = ?1
                      AND json_extract(metadata, '$.thread_type') = 'routine'
                      AND json_extract(metadata, '$.routine_id') = ?2
                    LIMIT 1
                    "#,
                    params![user_id, rid],
                )
                .await
                .map_err(|e| DatabaseError::Query(e.to_string()))?;

            if let Some(row) = rows
                .next()
                .await
                .map_err(|e| DatabaseError::Query(e.to_string()))?
            {
                return parse_uuid(get_text(&row, 0));
            }

            let id = Uuid::new_v4();
            let now = fmt_ts(&Utc::now());
            let metadata = serde_json::json!({
                "thread_type": "routine",
                "routine_id": routine_id.to_string(),
                "routine_name": routine_name,
            });
            conn.execute(
                "INSERT INTO conversations (id, channel, user_id, metadata, started_at, last_activity) VALUES (?1, ?2, ?3, ?4, ?5, ?5)",
                params![id.to_string(), "routine", user_id, metadata.to_string(), now],
            )
            .await
            .map_err(|e| DatabaseError::Query(e.to_string()))?;
            Ok(id)
        }
        .await;

    match &result {
        Ok(_) => {
            conn.execute("COMMIT", params![])
                .await
                .map_err(|e| DatabaseError::Query(e.to_string()))?;
        }
        Err(_) => {
            let _ = conn.execute("ROLLBACK", params![]).await;
        }
    }
    result
}

pub(super) async fn get_or_create_heartbeat_conversation(
    backend: &LibSqlBackend,
    user_id: &str,
) -> Result<Uuid, DatabaseError> {
    let conn = backend.connect().await?;

    conn.execute("BEGIN IMMEDIATE", params![])
        .await
        .map_err(|e| DatabaseError::Query(e.to_string()))?;

    let result: Result<Uuid, DatabaseError> = async {
            let mut rows = conn
                .query(
                    r#"
                    SELECT id FROM conversations
                    WHERE user_id = ?1 AND json_extract(metadata, '$.thread_type') = 'heartbeat'
                    LIMIT 1
                    "#,
                    params![user_id],
                )
                .await
                .map_err(|e| DatabaseError::Query(e.to_string()))?;

            if let Some(row) = rows
                .next()
                .await
                .map_err(|e| DatabaseError::Query(e.to_string()))?
            {
                return parse_uuid(get_text(&row, 0));
            }

            let id = Uuid::new_v4();
            let now = fmt_ts(&Utc::now());
            let metadata = serde_json::json!({ "thread_type": "heartbeat" });
            conn.execute(
                "INSERT INTO conversations (id, channel, user_id, metadata, started_at, last_activity) VALUES (?1, ?2, ?3, ?4, ?5, ?5)",
                params![id.to_string(), "heartbeat", user_id, metadata.to_string(), now],
            )
            .await
            .map_err(|e| DatabaseError::Query(e.to_string()))?;
            Ok(id)
        }
        .await;

    match &result {
        Ok(_) => {
            conn.execute("COMMIT", params![])
                .await
                .map_err(|e| DatabaseError::Query(e.to_string()))?;
        }
        Err(_) => {
            let _ = conn.execute("ROLLBACK", params![]).await;
        }
    }
    result
}

pub(super) async fn get_or_create_assistant_conversation(
    backend: &LibSqlBackend,
    user_id: &str,
    channel: &str,
) -> Result<Uuid, DatabaseError> {
    let conn = backend.connect().await?;
    let id = Uuid::new_v4();
    let now = fmt_ts(&Utc::now());
    let metadata = serde_json::json!({"thread_type": "assistant", "title": "Assistant"});
    conn.execute(
            "INSERT OR IGNORE INTO conversations (id, channel, user_id, metadata, started_at, last_activity) VALUES (?1, ?2, ?3, ?4, ?5, ?5)",
            params![id.to_string(), channel, user_id, metadata.to_string(), now],
        )
        .await
        .map_err(|e| DatabaseError::Query(e.to_string()))?;

    let mut rows = conn
        .query(
            r#"
                SELECT id FROM conversations
                WHERE user_id = ?1 AND channel = ?2
                  AND json_extract(metadata, '$.thread_type') = 'assistant'
                LIMIT 1
                "#,
            params![user_id, channel],
        )
        .await
        .map_err(|e| DatabaseError::Query(e.to_string()))?;

    let Some(row) = rows
        .next()
        .await
        .map_err(|e| DatabaseError::Query(e.to_string()))?
    else {
        return Err(DatabaseError::Query(
            "assistant conversation missing after insert-or-ignore".to_string(),
        ));
    };

    parse_uuid(get_text(&row, 0))
}
