use super::*;

pub(super) async fn add_conversation_message(
    backend: &LibSqlBackend,
    conversation_id: Uuid,
    role: &str,
    content: &str,
) -> Result<Uuid, DatabaseError> {
    let conn = backend.connect().await?;
    let id = Uuid::new_v4();
    let now = fmt_ts(&Utc::now());
    conn.execute(
            "INSERT INTO conversation_messages (id, conversation_id, role, content, created_at) VALUES (?1, ?2, ?3, ?4, ?5)",
            params![id.to_string(), conversation_id.to_string(), role, content, now],
        )
        .await
        .map_err(|e| DatabaseError::Query(e.to_string()))?;
    crud::touch_conversation(backend, conversation_id).await?;
    Ok(id)
}

pub(super) async fn list_conversation_messages_paginated(
    backend: &LibSqlBackend,
    conversation_id: Uuid,
    before: Option<(DateTime<Utc>, Uuid)>,
    limit: i64,
) -> Result<(Vec<ConversationMessage>, bool), DatabaseError> {
    let conn = backend.connect().await?;
    let fetch_limit = limit + 1;
    let cid = conversation_id.to_string();

    let mut rows = if let Some((before_ts, before_id)) = before {
        conn.query(
            r#"
                    SELECT id, role, content, created_at
                    FROM conversation_messages
                    WHERE conversation_id = ?1
                      AND (created_at < ?2 OR (created_at = ?2 AND id < ?3))
                    ORDER BY created_at DESC, id DESC, rowid DESC
                    LIMIT ?4
                    "#,
            params![cid, fmt_ts(&before_ts), before_id.to_string(), fetch_limit],
        )
        .await
    } else {
        conn.query(
            r#"
                    SELECT id, role, content, created_at
                    FROM conversation_messages
                    WHERE conversation_id = ?1
                    ORDER BY created_at DESC, id DESC, rowid DESC
                    LIMIT ?2
                    "#,
            params![cid, fetch_limit],
        )
        .await
    }
    .map_err(|e| DatabaseError::Query(e.to_string()))?;

    let mut all = Vec::new();
    while let Some(row) = rows
        .next()
        .await
        .map_err(|e| DatabaseError::Query(e.to_string()))?
    {
        all.push(ConversationMessage {
            id: get_text(&row, 0).parse().unwrap_or_default(),
            role: get_text(&row, 1),
            content: get_text(&row, 2),
            created_at: get_ts(&row, 3),
        });
    }

    let has_more = all.len() as i64 > limit;
    all.truncate(limit as usize);
    all.reverse();
    Ok((all, has_more))
}

pub(super) async fn list_conversation_messages(
    backend: &LibSqlBackend,
    conversation_id: Uuid,
) -> Result<Vec<ConversationMessage>, DatabaseError> {
    let conn = backend.connect().await?;
    let mut rows = conn
        .query(
            r#"
                SELECT id, role, content, created_at
                FROM conversation_messages
                WHERE conversation_id = ?1
                ORDER BY created_at ASC, rowid ASC
                "#,
            params![conversation_id.to_string()],
        )
        .await
        .map_err(|e| DatabaseError::Query(e.to_string()))?;

    let mut messages = Vec::new();
    while let Some(row) = rows
        .next()
        .await
        .map_err(|e| DatabaseError::Query(e.to_string()))?
    {
        messages.push(ConversationMessage {
            id: get_text(&row, 0).parse().unwrap_or_default(),
            role: get_text(&row, 1),
            content: get_text(&row, 2),
            created_at: get_ts(&row, 3),
        });
    }
    Ok(messages)
}
