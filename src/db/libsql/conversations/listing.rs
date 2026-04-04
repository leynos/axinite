//! LibSQL conversation-listing queries.
//!
//! These queries must return a stable top-N ordering, so the outer conversation
//! lists sort by `last_activity` and then `id` to break timestamp ties.

use super::*;

fn limit_to_i64(limit: usize) -> Result<i64, DatabaseError> {
    i64::try_from(limit).map_err(|error| {
        DatabaseError::Query(format!(
            "conversation list limit exceeds i64 range: {limit} ({error})"
        ))
    })
}

pub(super) async fn list_conversations_with_preview(
    backend: &LibSqlBackend,
    user_id: &str,
    channel: &str,
    limit: usize,
) -> Result<Vec<ConversationSummary>, DatabaseError> {
    let conn = backend.connect().await?;
    let limit = limit_to_i64(limit)?;
    let mut rows = conn
            .query(
                r#"
                SELECT
                    c.id,
                    c.started_at,
                    c.last_activity,
                    c.metadata,
                    c.channel,
                    (SELECT COUNT(*) FROM conversation_messages m WHERE m.conversation_id = c.id AND m.role = 'user') AS message_count,
                    (SELECT substr(m2.content, 1, 100)
                     FROM conversation_messages m2
                     WHERE m2.conversation_id = c.id AND m2.role = 'user'
                     ORDER BY m2.created_at ASC, m2.id ASC
                     LIMIT 1
                    ) AS title
                FROM conversations c
                WHERE c.user_id = ?1 AND c.channel = ?2
                ORDER BY datetime(c.last_activity) DESC, c.id DESC
                LIMIT ?3
                "#,
                params![user_id, channel, limit],
            )
            .await
            .map_err(|e| DatabaseError::Query(e.to_string()))?;

    let mut results = Vec::new();
    while let Some(row) = rows
        .next()
        .await
        .map_err(|e| DatabaseError::Query(e.to_string()))?
    {
        results.push(row_to_conversation_summary(&row)?);
    }
    Ok(results)
}

pub(super) async fn list_conversations_all_channels(
    backend: &LibSqlBackend,
    user_id: &str,
    limit: usize,
) -> Result<Vec<ConversationSummary>, DatabaseError> {
    let conn = backend.connect().await?;
    let limit = limit_to_i64(limit)?;
    let mut rows = conn
            .query(
                r#"
                SELECT
                    c.id,
                    c.started_at,
                    c.last_activity,
                    c.metadata,
                    c.channel,
                    (SELECT COUNT(*) FROM conversation_messages m WHERE m.conversation_id = c.id AND m.role = 'user') AS message_count,
                    (SELECT substr(m2.content, 1, 100)
                     FROM conversation_messages m2
                     WHERE m2.conversation_id = c.id AND m2.role = 'user'
                     ORDER BY m2.created_at ASC, m2.id ASC
                     LIMIT 1
                    ) AS title
                FROM conversations c
                WHERE c.user_id = ?1
                ORDER BY datetime(c.last_activity) DESC, c.id DESC
                LIMIT ?2
                "#,
                params![user_id, limit],
            )
            .await
            .map_err(|e| DatabaseError::Query(e.to_string()))?;

    let mut results = Vec::new();
    while let Some(row) = rows
        .next()
        .await
        .map_err(|e| DatabaseError::Query(e.to_string()))?
    {
        results.push(row_to_conversation_summary(&row)?);
    }
    Ok(results)
}
