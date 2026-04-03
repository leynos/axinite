use super::*;

async fn touch_conversation_with_connection(
    conn: &libsql::Connection,
    id: Uuid,
) -> Result<(), DatabaseError> {
    let now = fmt_ts(&Utc::now());
    conn.execute(
        "UPDATE conversations SET last_activity = ?2 WHERE id = ?1",
        params![id.to_string(), now],
    )
    .await
    .map_err(|e| DatabaseError::Query(e.to_string()))?;
    Ok(())
}

pub(super) async fn add_conversation_message(
    backend: &LibSqlBackend,
    conversation_id: Uuid,
    role: &str,
    content: &str,
) -> Result<Uuid, DatabaseError> {
    let conn = backend.connect().await?;
    let id = Uuid::new_v4();
    let now = fmt_ts(&Utc::now());
    let tx = conn
        .transaction()
        .await
        .map_err(|e| DatabaseError::Query(e.to_string()))?;
    tx.execute(
            "INSERT INTO conversation_messages (id, conversation_id, role, content, created_at) VALUES (?1, ?2, ?3, ?4, ?5)",
            params![id.to_string(), conversation_id.to_string(), role, content, now],
        )
        .await
        .map_err(|e| DatabaseError::Query(e.to_string()))?;
    touch_conversation_with_connection(&tx, conversation_id).await?;
    tx.commit()
        .await
        .map_err(|e| DatabaseError::Query(e.to_string()))?;
    Ok(id)
}

pub(super) async fn list_conversation_messages_paginated(
    backend: &LibSqlBackend,
    conversation_id: Uuid,
    before: Option<(DateTime<Utc>, Uuid)>,
    limit: usize,
) -> Result<(Vec<ConversationMessage>, bool), DatabaseError> {
    let conn = backend.connect().await?;
    let fetch_limit = limit.checked_add(1).ok_or_else(|| {
        DatabaseError::Query("conversation message pagination limit overflow".to_string())
    })?;
    let cid = conversation_id.to_string();
    let fetch_limit_i64 = i64::try_from(fetch_limit).map_err(|_| {
        DatabaseError::Query("conversation message pagination limit overflow".to_string())
    })?;

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
            params![
                cid,
                fmt_ts(&before_ts),
                before_id.to_string(),
                fetch_limit_i64
            ],
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
            params![cid, fetch_limit_i64],
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
        let id = parse_uuid(get_text(&row, 0))?;
        all.push(ConversationMessage {
            id,
            role: get_text(&row, 1),
            content: get_text(&row, 2),
            created_at: get_ts(&row, 3),
        });
    }

    let has_more = all.len() > limit;
    all.truncate(limit);
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
        let id = parse_uuid(get_text(&row, 0))?;
        messages.push(ConversationMessage {
            id,
            role: get_text(&row, 1),
            content: get_text(&row, 2),
            created_at: get_ts(&row, 3),
        });
    }
    Ok(messages)
}
