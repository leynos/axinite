use super::*;

pub(super) async fn create_conversation(
    backend: &LibSqlBackend,
    channel: &str,
    user_id: &str,
    thread_id: Option<&str>,
) -> Result<Uuid, DatabaseError> {
    let conn = backend.connect().await?;
    let id = Uuid::new_v4();
    let now = fmt_ts(&Utc::now());
    conn.execute(
            "INSERT INTO conversations (id, channel, user_id, thread_id, started_at, last_activity) VALUES (?1, ?2, ?3, ?4, ?5, ?5)",
            params![id.to_string(), channel, user_id, opt_text(thread_id), now],
        )
        .await
        .map_err(|e| DatabaseError::Query(e.to_string()))?;
    Ok(id)
}

pub(super) async fn touch_conversation(
    backend: &LibSqlBackend,
    id: Uuid,
) -> Result<(), DatabaseError> {
    let conn = backend.connect().await?;
    let now = fmt_ts(&Utc::now());
    conn.execute(
        "UPDATE conversations SET last_activity = ?2 WHERE id = ?1",
        params![id.to_string(), now],
    )
    .await
    .map_err(|e| DatabaseError::Query(e.to_string()))?;
    Ok(())
}

pub(super) async fn ensure_conversation(
    backend: &LibSqlBackend,
    params: EnsureConversationParams<'_>,
) -> Result<(), DatabaseError> {
    let EnsureConversationParams {
        id,
        channel,
        user_id,
        thread_id,
    } = params;
    let conn = backend.connect().await?;
    let now = fmt_ts(&Utc::now());
    conn.execute(
        r#"
            INSERT INTO conversations (id, channel, user_id, thread_id, started_at, last_activity)
            VALUES (?1, ?2, ?3, ?4, ?5, ?5)
            ON CONFLICT (id) DO UPDATE SET last_activity = ?5
            "#,
        params![id.to_string(), channel, user_id, opt_text(thread_id), now],
    )
    .await
    .map_err(|e| DatabaseError::Query(e.to_string()))?;
    Ok(())
}

pub(super) async fn create_conversation_with_metadata(
    backend: &LibSqlBackend,
    channel: &str,
    user_id: &str,
    metadata: &serde_json::Value,
) -> Result<Uuid, DatabaseError> {
    let conn = backend.connect().await?;
    let id = Uuid::new_v4();
    let now = fmt_ts(&Utc::now());
    conn.execute(
            "INSERT INTO conversations (id, channel, user_id, metadata, started_at, last_activity) VALUES (?1, ?2, ?3, ?4, ?5, ?5)",
            params![id.to_string(), channel, user_id, metadata.to_string(), now],
        )
        .await
        .map_err(|e| DatabaseError::Query(e.to_string()))?;
    Ok(id)
}
