//! Conversation preview list queries and title resolution.

use super::{ConversationSummary, Store};
use crate::error::DatabaseError;

pub(super) fn resolve_preview_title(
    metadata: &serde_json::Value,
    sql_title: Option<String>,
) -> Option<String> {
    sql_title
        .or_else(|| {
            metadata
                .get("title")
                .and_then(|value| value.as_str())
                .map(String::from)
        })
        .or_else(|| {
            metadata
                .get("routine_name")
                .and_then(|value| value.as_str())
                .map(String::from)
        })
}

#[cfg(feature = "postgres")]
impl Store {
    /// List conversations with a title derived from the first user message.
    pub async fn list_conversations_with_preview(
        &self,
        user_id: &str,
        channel: &str,
        limit: i64,
    ) -> Result<Vec<ConversationSummary>, DatabaseError> {
        let conn = self.conn().await?;
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
                     ORDER BY m2.created_at ASC
                     LIMIT 1
                    ) AS title
                FROM conversations c
                WHERE c.user_id = $1 AND c.channel = $2
                ORDER BY c.last_activity DESC
                LIMIT $3
                "#,
                &[&user_id, &channel, &limit],
            )
            .await?;

        Ok(rows
            .iter()
            .map(|r| {
                let metadata: serde_json::Value = r.get("metadata");
                let thread_type = metadata
                    .get("thread_type")
                    .and_then(|v| v.as_str())
                    .map(String::from);
                let title = resolve_preview_title(&metadata, r.get("title"));
                ConversationSummary {
                    id: r.get("id"),
                    title,
                    message_count: r.get("message_count"),
                    started_at: r.get("started_at"),
                    last_activity: r.get("last_activity"),
                    thread_type,
                    channel: r.get("channel"),
                }
            })
            .collect())
    }

    /// List conversations across all channels with a title derived from the first user message.
    pub async fn list_conversations_all_channels(
        &self,
        user_id: &str,
        limit: i64,
    ) -> Result<Vec<ConversationSummary>, DatabaseError> {
        let conn = self.conn().await?;
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
                     ORDER BY m2.created_at ASC
                     LIMIT 1
                    ) AS title
                FROM conversations c
                WHERE c.user_id = $1
                ORDER BY c.last_activity DESC
                LIMIT $2
                "#,
                &[&user_id, &limit],
            )
            .await?;

        Ok(rows
            .iter()
            .map(|r| {
                let metadata: serde_json::Value = r.get("metadata");
                let thread_type = metadata
                    .get("thread_type")
                    .and_then(|v| v.as_str())
                    .map(String::from);
                let title = resolve_preview_title(&metadata, r.get("title"));
                ConversationSummary {
                    id: r.get("id"),
                    title,
                    message_count: r.get("message_count"),
                    started_at: r.get("started_at"),
                    last_activity: r.get("last_activity"),
                    thread_type,
                    channel: r.get("channel"),
                }
            })
            .collect())
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::resolve_preview_title;

    #[test]
    fn preview_title_prefers_metadata_title_before_routine_name() {
        let metadata = json!({
            "title": "Assistant",
            "routine_name": "daily-standup",
        });

        assert_eq!(
            resolve_preview_title(&metadata, None),
            Some("Assistant".to_string())
        );
    }

    #[test]
    fn preview_title_prefers_sql_title_over_metadata() {
        let metadata = json!({
            "title": "Assistant",
            "routine_name": "daily-standup",
        });

        assert_eq!(
            resolve_preview_title(&metadata, Some("First user message".to_string())),
            Some("First user message".to_string())
        );
    }
}
