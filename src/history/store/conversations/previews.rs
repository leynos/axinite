//! Conversation preview list queries and title resolution.

#[cfg(feature = "postgres")]
use tokio_postgres::Row;

use super::{ConversationSummary, Store};
use crate::error::DatabaseError;
use crate::history::preview_title_from_metadata;

pub(super) fn resolve_preview_title(
    metadata: &serde_json::Value,
    sql_title: Option<String>,
) -> Option<String> {
    preview_title_from_metadata(metadata, sql_title)
}

#[cfg(feature = "postgres")]
fn row_to_conversation_summary(row: &Row) -> ConversationSummary {
    let metadata: serde_json::Value = row.get("metadata");
    let thread_type = metadata
        .get("thread_type")
        .and_then(|value| value.as_str())
        .map(String::from);
    let title = resolve_preview_title(&metadata, row.get("title"));

    ConversationSummary {
        id: row.get("id"),
        title,
        message_count: row.get("message_count"),
        started_at: row.get("started_at"),
        last_activity: row.get("last_activity"),
        thread_type,
        channel: row.get("channel"),
    }
}

#[cfg(feature = "postgres")]
impl Store {
    /// List conversations with a title derived from the first user message.
    pub async fn list_conversations_with_preview(
        &self,
        user_id: &str,
        channel: &str,
        limit: usize,
    ) -> Result<Vec<ConversationSummary>, DatabaseError> {
        let conn = self.conn().await?;
        let limit = i64::try_from(limit)
            .map_err(|_| DatabaseError::Query("conversation preview limit overflow".to_string()))?;
        let rows = conn
            .query(
                r#"
                SELECT
                    c.id,
                    c.started_at,
                    c.last_activity,
                    c.metadata,
                    c.channel,
                    (SELECT COUNT(*) FROM conversation_messages m WHERE m.conversation_id = c.id AND m.role = 'user') AS message_count,
                     (SELECT LEFT(m2.content, 100)
                     FROM conversation_messages m2
                     WHERE m2.conversation_id = c.id AND m2.role = 'user'
                     ORDER BY m2.created_at ASC, m2.id ASC
                     LIMIT 1
                    ) AS title
                FROM conversations c
                WHERE c.user_id = $1 AND c.channel = $2
                ORDER BY c.last_activity DESC, c.id DESC
                LIMIT $3
                "#,
                &[&user_id, &channel, &limit],
            )
            .await?;

        Ok(rows.iter().map(row_to_conversation_summary).collect())
    }

    /// List conversations across all channels with a title derived from the first user message.
    pub async fn list_conversations_all_channels(
        &self,
        user_id: &str,
        limit: usize,
    ) -> Result<Vec<ConversationSummary>, DatabaseError> {
        let conn = self.conn().await?;
        let limit = i64::try_from(limit)
            .map_err(|_| DatabaseError::Query("conversation preview limit overflow".to_string()))?;
        let rows = conn
            .query(
                r#"
                SELECT
                    c.id,
                    c.started_at,
                    c.last_activity,
                    c.metadata,
                    c.channel,
                    (SELECT COUNT(*) FROM conversation_messages m WHERE m.conversation_id = c.id AND m.role = 'user') AS message_count,
                     (SELECT LEFT(m2.content, 100)
                     FROM conversation_messages m2
                     WHERE m2.conversation_id = c.id AND m2.role = 'user'
                     ORDER BY m2.created_at ASC, m2.id ASC
                     LIMIT 1
                    ) AS title
                FROM conversations c
                WHERE c.user_id = $1
                ORDER BY c.last_activity DESC, c.id DESC
                LIMIT $2
                "#,
                &[&user_id, &limit],
            )
            .await?;

        Ok(rows.iter().map(row_to_conversation_summary).collect())
    }
}
