//! LibSQL conversation-message write and read helpers.
//!
//! This module owns atomic message inserts plus cursor-based and full-history
//! reads. Message ordering is monotonic by `(created_at, id)` with `rowid` only
//! as a final SQLite tiebreaker where needed, so cursor pagination and full
//! reads stay consistent.

use chrono::{DateTime, Utc};
use libsql::params;
use uuid::Uuid;

use super::{
    ConversationMessage, DatabaseError, LibSqlBackend, fmt_ts, get_text, get_ts, parse_uuid,
};

fn row_to_conversation_message(row: &libsql::Row) -> Result<ConversationMessage, DatabaseError> {
    Ok(ConversationMessage {
        id: parse_uuid(get_text(row, 0))?,
        role: get_text(row, 1),
        content: get_text(row, 2),
        created_at: get_ts(row, 3),
    })
}

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
    if limit == 0 {
        return Err(DatabaseError::Validation(
            "conversation message pagination limit must be > 0".to_string(),
        ));
    }
    let conn = backend.connect().await?;
    let fetch_limit = limit.checked_add(1).ok_or_else(|| {
        DatabaseError::Validation("conversation message pagination limit overflow".to_string())
    })?;
    let cid = conversation_id.to_string();
    let fetch_limit_i64 = i64::try_from(fetch_limit).map_err(|_| {
        DatabaseError::Validation("conversation message pagination limit overflow".to_string())
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
        all.push(row_to_conversation_message(&row)?);
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
                ORDER BY created_at ASC, id ASC, rowid ASC
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
        messages.push(row_to_conversation_message(&row)?);
    }
    Ok(messages)
}

pub(super) async fn list_conversation_messages_scoped(
    backend: &LibSqlBackend,
    conversation_id: Uuid,
    user_id: &str,
    channel: &str,
) -> Result<Vec<ConversationMessage>, DatabaseError> {
    let conn = backend.connect().await?;
    let mut rows = conn
        .query(
            "SELECT 1 FROM conversations WHERE id = ?1 AND user_id = ?2 AND channel = ?3",
            params![conversation_id.to_string(), user_id, channel],
        )
        .await
        .map_err(|e| DatabaseError::Query(e.to_string()))?;

    let found = rows
        .next()
        .await
        .map_err(|e| DatabaseError::Query(e.to_string()))?;
    if found.is_none() {
        return Err(DatabaseError::NotFound {
            entity: "conversation".to_string(),
            id: conversation_id.to_string(),
        });
    }

    list_conversation_messages(backend, conversation_id).await
}

#[cfg(test)]
mod tests {
    use uuid::Uuid;

    use crate::db::Database;
    use crate::db::libsql::LibSqlBackend;
    use crate::error::DatabaseError;

    async fn in_memory_backend() -> LibSqlBackend {
        let backend = LibSqlBackend::new_memory()
            .await
            .expect("in-memory backend creation");
        backend.run_migrations().await.expect("migrations");
        backend
    }

    #[tokio::test]
    async fn test_zero_limit_rejected() {
        let backend = in_memory_backend().await;
        let err = super::list_conversation_messages_paginated(&backend, Uuid::new_v4(), None, 0)
            .await
            .expect_err("zero limit should be rejected");

        assert!(
            matches!(err, DatabaseError::Validation(ref msg) if msg.contains("must be > 0")),
            "expected Validation error for zero limit, got: {err:?}"
        );
    }

    #[tokio::test]
    async fn test_usize_max_limit_rejected() {
        let backend = in_memory_backend().await;
        let err =
            super::list_conversation_messages_paginated(&backend, Uuid::new_v4(), None, usize::MAX)
                .await
                .expect_err("usize::MAX limit should be rejected");

        assert!(
            matches!(err, DatabaseError::Validation(ref msg) if msg.contains("overflow")),
            "expected overflow Validation error, got: {err:?}"
        );
    }

    #[tokio::test]
    async fn test_limit_exceeding_i64_range_rejected() {
        let backend = in_memory_backend().await;
        // i64::MAX as usize: passes checked_add(1) on 64-bit but fails
        // i64::try_from because the result exceeds i64::MAX.
        let limit = i64::MAX as usize;
        let err =
            super::list_conversation_messages_paginated(&backend, Uuid::new_v4(), None, limit)
                .await
                .expect_err("limit exceeding i64 range should be rejected");

        assert!(
            matches!(err, DatabaseError::Validation(ref msg) if msg.contains("overflow")),
            "expected overflow Validation error, got: {err:?}"
        );
    }
}
