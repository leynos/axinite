//! LibSQL singleton-conversation helpers for routine, heartbeat, and assistant
//! threads. Routine and heartbeat helpers open an immediate transaction so the
//! lookup and optional insert happen atomically on one connection, while the
//! assistant helper relies on insert-or-ignore plus a follow-up read.

use super::*;

/// How to find, or else create, one singleton conversation.
struct SingletonSpec<'a> {
    /// A query selecting the conversation `id`, bound to `select_params`.
    select_sql: &'a str,
    select_params: Vec<libsql::Value>,
    user_id: &'a str,
    channel: &'a str,
    /// Metadata stored on the conversation when it is created.
    metadata: serde_json::Value,
}

/// Find the conversation `spec` describes, inserting it when absent.
async fn find_or_insert_singleton(
    conn: &libsql::Connection,
    spec: &SingletonSpec<'_>,
) -> Result<Uuid, DatabaseError> {
    let mut rows = conn
        .query(
            spec.select_sql,
            libsql::params::Params::Positional(spec.select_params.clone()),
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
    conn.execute(
        "INSERT INTO conversations (id, channel, user_id, metadata, started_at, last_activity) VALUES (?1, ?2, ?3, ?4, ?5, ?5)",
        params![id.to_string(), spec.channel, spec.user_id, spec.metadata.to_string(), now],
    )
    .await
    .map_err(|e| DatabaseError::Query(e.to_string()))?;
    Ok(id)
}

/// Run [`find_or_insert_singleton`] inside an immediate transaction, so the
/// lookup and the insert are atomic.
async fn get_or_create_singleton(
    backend: &LibSqlBackend,
    spec: SingletonSpec<'_>,
) -> Result<Uuid, DatabaseError> {
    let conn = backend.connect().await?;

    conn.execute("BEGIN IMMEDIATE", params![])
        .await
        .map_err(|e| DatabaseError::Query(e.to_string()))?;

    let result = find_or_insert_singleton(&conn, &spec).await;

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

pub(super) async fn get_or_create_routine_conversation(
    backend: &LibSqlBackend,
    routine_id: Uuid,
    routine_name: &str,
    user_id: &str,
) -> Result<Uuid, DatabaseError> {
    get_or_create_singleton(
        backend,
        SingletonSpec {
            select_sql: r#"
                SELECT id FROM conversations
                WHERE user_id = ?1
                  AND json_extract(metadata, '$.thread_type') = 'routine'
                  AND json_extract(metadata, '$.routine_id') = ?2
                LIMIT 1
                "#,
            select_params: vec![user_id.into(), routine_id.to_string().into()],
            user_id,
            channel: "routine",
            metadata: serde_json::json!({
                "thread_type": "routine",
                "routine_id": routine_id.to_string(),
                "routine_name": routine_name,
            }),
        },
    )
    .await
}

pub(super) async fn get_or_create_heartbeat_conversation(
    backend: &LibSqlBackend,
    user_id: &str,
) -> Result<Uuid, DatabaseError> {
    get_or_create_singleton(
        backend,
        SingletonSpec {
            select_sql: r#"
                SELECT id FROM conversations
                WHERE user_id = ?1 AND json_extract(metadata, '$.thread_type') = 'heartbeat'
                LIMIT 1
                "#,
            select_params: vec![user_id.into()],
            user_id,
            channel: "heartbeat",
            metadata: serde_json::json!({ "thread_type": "heartbeat" }),
        },
    )
    .await
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
