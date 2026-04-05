//! Conversation persistence types and core CRUD helpers.

use chrono::{DateTime, Utc};
#[cfg(feature = "postgres")]
use deadpool_postgres::GenericClient;
use uuid::Uuid;

#[cfg(feature = "postgres")]
#[path = "conversations/metadata.rs"]
mod metadata;
#[cfg(feature = "postgres")]
#[path = "conversations/previews.rs"]
mod previews;
#[cfg(feature = "postgres")]
#[path = "conversations/singletons.rs"]
mod singletons;

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
    /// Preview title: typically the first user message, truncated to 100 chars.
    ///
    /// Falls back to metadata-backed titles such as `title` or
    /// `routine_name` when no user message is available.
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
pub(super) async fn touch_conversation_with_client(
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
        before: Option<(DateTime<Utc>, Uuid)>,
        limit: i64,
    ) -> Result<(Vec<ConversationMessage>, bool), DatabaseError> {
        if limit <= 0 {
            return Ok((Vec::new(), false));
        }

        let conn = self.conn().await?;
        let fetch_limit = limit.checked_add(1).ok_or_else(|| {
            DatabaseError::Query("conversation message pagination limit overflow".to_string())
        })?;

        let rows = if let Some((before_ts, before_id)) = before {
            conn.query(
                r#"
                SELECT id, role, content, created_at
                FROM conversation_messages
                WHERE conversation_id = $1
                  AND (created_at < $2 OR (created_at = $2 AND id < $3))
                ORDER BY created_at DESC, id DESC
                LIMIT $4
                "#,
                &[&conversation_id, &before_ts, &before_id, &fetch_limit],
            )
            .await?
        } else {
            conn.query(
                r#"
                SELECT id, role, content, created_at
                FROM conversation_messages
                WHERE conversation_id = $1
                ORDER BY created_at DESC, id DESC
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
                ORDER BY created_at ASC, id ASC
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
}
