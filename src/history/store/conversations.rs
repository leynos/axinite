//! Conversation persistence types and helpers.

use chrono::{DateTime, Utc};
#[cfg(feature = "postgres")]
use deadpool_postgres::GenericClient;
use uuid::Uuid;

#[cfg(feature = "postgres")]
use super::Store;
#[cfg(feature = "postgres")]
use crate::db::EnsureConversationParams;
#[cfg(feature = "postgres")]
use crate::error::DatabaseError;

/// Summary of a conversation for the thread list.
#[derive(Debug, Clone)]
pub struct ConversationSummary {
    pub id: Uuid,
    /// First user message, truncated to 100 chars.
    pub title: Option<String>,
    pub message_count: i64,
    pub started_at: DateTime<Utc>,
    pub last_activity: DateTime<Utc>,
    /// Thread type extracted from metadata (e.g. "assistant", "thread").
    pub thread_type: Option<String>,
    /// Channel that owns this conversation (e.g. "gateway", "telegram", "routine").
    pub channel: String,
}

/// A single message in a conversation.
#[derive(Debug, Clone)]
pub struct ConversationMessage {
    pub id: Uuid,
    pub role: String,
    pub content: String,
    pub created_at: DateTime<Utc>,
}

#[cfg(feature = "postgres")]
fn preview_title(metadata: &serde_json::Value, sql_title: Option<String>) -> Option<String> {
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
async fn touch_conversation_with_client(
    client: &impl GenericClient,
    id: Uuid,
) -> Result<(), DatabaseError> {
    client
        .execute(
            "UPDATE conversations SET last_activity = NOW() WHERE id = $1",
            &[&id],
        )
        .await?;
    Ok(())
}

#[cfg(feature = "postgres")]
impl Store {
    /// Create a new conversation.
    pub async fn create_conversation(
        &self,
        channel: &str,
        user_id: &str,
        thread_id: Option<&str>,
    ) -> Result<Uuid, DatabaseError> {
        let conn = self.conn().await?;
        let id = Uuid::new_v4();

        conn.execute(
            "INSERT INTO conversations (id, channel, user_id, thread_id) VALUES ($1, $2, $3, $4)",
            &[&id, &channel, &user_id, &thread_id],
        )
        .await?;

        Ok(id)
    }

    /// Update conversation last activity.
    pub async fn touch_conversation(&self, id: Uuid) -> Result<(), DatabaseError> {
        let conn = self.conn().await?;
        touch_conversation_with_client(&conn, id).await
    }

    /// Add a message to a conversation.
    pub async fn add_conversation_message(
        &self,
        conversation_id: Uuid,
        role: &str,
        content: &str,
    ) -> Result<Uuid, DatabaseError> {
        let mut conn = self.conn().await?;
        let id = Uuid::new_v4();
        let tx = conn.transaction().await?;

        tx.execute(
            "INSERT INTO conversation_messages (id, conversation_id, role, content) VALUES ($1, $2, $3, $4)",
            &[&id, &conversation_id, &role, &content],
        )
        .await?;

        touch_conversation_with_client(&tx, conversation_id).await?;
        tx.commit().await?;

        Ok(id)
    }

    /// Ensure a conversation row exists for a given UUID.
    pub async fn ensure_conversation(
        &self,
        params: EnsureConversationParams<'_>,
    ) -> Result<(), DatabaseError> {
        let EnsureConversationParams {
            id,
            channel,
            user_id,
            thread_id,
        } = params;
        let conn = self.conn().await?;
        conn.execute(
            r#"
            INSERT INTO conversations (id, channel, user_id, thread_id)
            VALUES ($1, $2, $3, $4)
            ON CONFLICT (id) DO UPDATE SET last_activity = NOW()
            "#,
            &[&id, &channel, &user_id, &thread_id],
        )
        .await?;
        Ok(())
    }

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
                let sql_title: Option<String> = r.get("title");
                let title = preview_title(&metadata, sql_title);
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
                let sql_title: Option<String> = r.get("title");
                let title = preview_title(&metadata, sql_title);
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

    /// Get or create a persistent conversation for a routine.
    pub async fn get_or_create_routine_conversation(
        &self,
        routine_id: Uuid,
        routine_name: &str,
        user_id: &str,
    ) -> Result<Uuid, DatabaseError> {
        let conn = self.conn().await?;
        let rid = routine_id.to_string();
        let new_id = Uuid::new_v4();
        let metadata = serde_json::json!({
            "thread_type": "routine",
            "routine_id": routine_id.to_string(),
            "routine_name": routine_name,
        });
        conn.execute(
            r#"
            INSERT INTO conversations (id, channel, user_id, metadata)
            VALUES ($1, 'routine', $2, $3)
            ON CONFLICT (user_id, (metadata->>'routine_id'))
                WHERE metadata->>'routine_id' IS NOT NULL
                DO NOTHING
            "#,
            &[&new_id, &user_id, &metadata],
        )
        .await?;

        let row = conn
            .query_one(
                r#"
                SELECT id FROM conversations
                WHERE user_id = $1 AND metadata->>'routine_id' = $2
                LIMIT 1
                "#,
                &[&user_id, &rid],
            )
            .await?;

        Ok(row.get("id"))
    }

    /// Get or create the singleton heartbeat conversation for a user.
    pub async fn get_or_create_heartbeat_conversation(
        &self,
        user_id: &str,
    ) -> Result<Uuid, DatabaseError> {
        let conn = self.conn().await?;
        let new_id = Uuid::new_v4();
        let metadata = serde_json::json!({
            "thread_type": "heartbeat",
        });
        conn.execute(
            r#"
            INSERT INTO conversations (id, channel, user_id, metadata)
            VALUES ($1, 'heartbeat', $2, $3)
            ON CONFLICT (user_id)
                WHERE metadata->>'thread_type' = 'heartbeat'
                DO NOTHING
            "#,
            &[&new_id, &user_id, &metadata],
        )
        .await?;

        let row = conn
            .query_one(
                r#"
                SELECT id FROM conversations
                WHERE user_id = $1 AND metadata->>'thread_type' = 'heartbeat'
                LIMIT 1
                "#,
                &[&user_id],
            )
            .await?;

        Ok(row.get("id"))
    }

    /// Get or create the singleton "assistant" conversation for a user+channel.
    pub async fn get_or_create_assistant_conversation(
        &self,
        user_id: &str,
        channel: &str,
    ) -> Result<Uuid, DatabaseError> {
        let conn = self.conn().await?;
        let id = Uuid::new_v4();
        let metadata = serde_json::json!({"thread_type": "assistant", "title": "Assistant"});
        conn.execute(
            r#"
            INSERT INTO conversations (id, channel, user_id, metadata)
            VALUES ($1, $2, $3, $4)
            ON CONFLICT (user_id, channel)
                WHERE metadata->>'thread_type' = 'assistant'
                DO NOTHING
            "#,
            &[&id, &channel, &user_id, &metadata],
        )
        .await?;

        let row = conn
            .query_one(
                r#"
                SELECT id FROM conversations
                WHERE user_id = $1 AND channel = $2 AND metadata->>'thread_type' = 'assistant'
                LIMIT 1
                "#,
                &[&user_id, &channel],
            )
            .await?;

        Ok(row.get("id"))
    }

    /// Create a conversation with specific metadata.
    pub async fn create_conversation_with_metadata(
        &self,
        channel: &str,
        user_id: &str,
        metadata: &serde_json::Value,
    ) -> Result<Uuid, DatabaseError> {
        let conn = self.conn().await?;
        let id = Uuid::new_v4();

        conn.execute(
            "INSERT INTO conversations (id, channel, user_id, metadata) VALUES ($1, $2, $3, $4)",
            &[&id, &channel, &user_id, metadata],
        )
        .await?;

        Ok(id)
    }

    /// Check whether a conversation belongs to the given user.
    pub async fn conversation_belongs_to_user(
        &self,
        conversation_id: Uuid,
        user_id: &str,
    ) -> Result<bool, DatabaseError> {
        let conn = self.conn().await?;
        let row = conn
            .query_opt(
                "SELECT 1 FROM conversations WHERE id = $1 AND user_id = $2",
                &[&conversation_id, &user_id],
            )
            .await?;
        Ok(row.is_some())
    }

    /// Load messages for a conversation with cursor-based pagination.
    pub async fn list_conversation_messages_paginated(
        &self,
        conversation_id: Uuid,
        before: Option<DateTime<Utc>>,
        limit: i64,
    ) -> Result<(Vec<ConversationMessage>, bool), DatabaseError> {
        let conn = self.conn().await?;
        let fetch_limit = limit + 1;

        let rows = if let Some(before_ts) = before {
            conn.query(
                r#"
                SELECT id, role, content, created_at
                FROM conversation_messages
                WHERE conversation_id = $1 AND created_at < $2
                ORDER BY created_at DESC
                LIMIT $3
                "#,
                &[&conversation_id, &before_ts, &fetch_limit],
            )
            .await?
        } else {
            conn.query(
                r#"
                SELECT id, role, content, created_at
                FROM conversation_messages
                WHERE conversation_id = $1
                ORDER BY created_at DESC
                LIMIT $2
                "#,
                &[&conversation_id, &fetch_limit],
            )
            .await?
        };

        let has_more = rows.len() as i64 > limit;
        let take_count = (rows.len() as i64).min(limit) as usize;

        let mut messages: Vec<ConversationMessage> = rows
            .iter()
            .take(take_count)
            .map(|r| ConversationMessage {
                id: r.get("id"),
                role: r.get("role"),
                content: r.get("content"),
                created_at: r.get("created_at"),
            })
            .collect();
        messages.reverse();

        Ok((messages, has_more))
    }

    /// Merge a single key into a conversation's metadata JSONB.
    pub async fn update_conversation_metadata_field(
        &self,
        id: Uuid,
        key: &str,
        value: &serde_json::Value,
    ) -> Result<(), DatabaseError> {
        let conn = self.conn().await?;
        let patch = serde_json::json!({ key: value });
        conn.execute(
            "UPDATE conversations SET metadata = COALESCE(metadata, '{}'::jsonb) || $2 WHERE id = $1",
            &[&id, &patch],
        )
        .await?;
        Ok(())
    }

    /// Read the metadata JSONB for a conversation.
    pub async fn get_conversation_metadata(
        &self,
        id: Uuid,
    ) -> Result<Option<serde_json::Value>, DatabaseError> {
        let conn = self.conn().await?;
        let row = conn
            .query_opt("SELECT metadata FROM conversations WHERE id = $1", &[&id])
            .await?;
        Ok(row.map(|r| r.get::<_, serde_json::Value>(0)))
    }

    /// Load all messages for a conversation, ordered chronologically.
    pub async fn list_conversation_messages(
        &self,
        conversation_id: Uuid,
    ) -> Result<Vec<ConversationMessage>, DatabaseError> {
        let conn = self.conn().await?;
        let rows = conn
            .query(
                r#"
                SELECT id, role, content, created_at
                FROM conversation_messages
                WHERE conversation_id = $1
                ORDER BY created_at ASC
                "#,
                &[&conversation_id],
            )
            .await?;

        Ok(rows
            .iter()
            .map(|r| ConversationMessage {
                id: r.get("id"),
                role: r.get("role"),
                content: r.get("content"),
                created_at: r.get("created_at"),
            })
            .collect())
    }
}

#[cfg(test)]
mod tests {
    use chrono::Utc;
    use uuid::Uuid;

    use super::*;

    #[test]
    fn test_conversation_summary_has_channel_field() {
        let summary = ConversationSummary {
            id: Uuid::nil(),
            title: Some("Hello".to_string()),
            message_count: 1,
            started_at: Utc::now(),
            last_activity: Utc::now(),
            thread_type: Some("thread".to_string()),
            channel: "telegram".to_string(),
        };
        assert_eq!(summary.channel, "telegram");
    }

    #[test]
    fn test_conversation_summary_channel_various_values() {
        for ch in ["gateway", "routine", "heartbeat", "telegram", "signal"] {
            let summary = ConversationSummary {
                id: Uuid::nil(),
                title: None,
                message_count: 0,
                started_at: Utc::now(),
                last_activity: Utc::now(),
                thread_type: None,
                channel: ch.to_string(),
            };
            assert_eq!(summary.channel, ch);
        }
    }

    #[cfg(feature = "postgres")]
    #[test]
    fn preview_title_prefers_metadata_title_before_routine_name() {
        use serde_json::json;

        let metadata = json!({
            "title": "Assistant",
            "routine_name": "daily-standup",
        });

        assert_eq!(
            preview_title(&metadata, None),
            Some("Assistant".to_string())
        );
    }
}
