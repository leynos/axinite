//! Singleton and routine conversation helpers.

use uuid::Uuid;

use super::Store;
use crate::error::DatabaseError;

#[cfg(feature = "postgres")]
impl Store {
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
        let row = conn
            .query_one(
                r#"
                INSERT INTO conversations (id, channel, user_id, metadata)
                VALUES ($1, $2, $3, $4)
                ON CONFLICT (user_id, channel)
                    WHERE metadata->>'thread_type' = 'assistant'
                    DO UPDATE SET metadata = conversations.metadata
                RETURNING id
                "#,
                &[&id, &channel, &user_id, &metadata],
            )
            .await?;

        Ok(row.get("id"))
    }
}
