//! Singleton and routine conversation helpers.
//!
//! "Singleton" conversations are stable per-scope threads, such as one
//! assistant conversation per `(user_id, channel)` pair or one heartbeat
//! conversation per user. These helpers are idempotent: repeated calls for the
//! same scope return the same persisted conversation id.

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

#[cfg(all(test, feature = "postgres"))]
mod tests {
    use rstest::{fixture, rstest};
    use uuid::Uuid;

    use super::*;
    use crate::testing::postgres::try_test_pg_db;

    #[fixture]
    async fn store() -> Option<Store> {
        let backend = try_test_pg_db().await?;
        Some(Store::from_pool(backend.pool()))
    }

    async fn cleanup_user(store: &Store, user_id: &str) {
        let conn = store.conn().await.expect("connection should be available");
        conn.execute("DELETE FROM conversations WHERE user_id = $1", &[&user_id])
            .await
            .expect("conversations should be deleted");
    }

    #[rstest]
    #[tokio::test]
    async fn routine_singleton_is_idempotent(#[future] store: Option<Store>) {
        let Some(store) = store.await else { return };
        let user_id = format!("routine-singleton-{}", Uuid::new_v4());
        let routine_id = Uuid::new_v4();

        let first = store
            .get_or_create_routine_conversation(routine_id, "daily-standup", &user_id)
            .await
            .expect("first routine singleton lookup should succeed");
        let second = store
            .get_or_create_routine_conversation(routine_id, "daily-standup", &user_id)
            .await
            .expect("second routine singleton lookup should succeed");

        assert_eq!(first, second);

        let metadata = store
            .get_conversation_metadata(first)
            .await
            .expect("metadata query should succeed")
            .expect("metadata should exist");
        assert_eq!(metadata["thread_type"], "routine");
        assert_eq!(metadata["routine_id"], routine_id.to_string());
        assert_eq!(metadata["routine_name"], "daily-standup");

        cleanup_user(&store, &user_id).await;
    }

    #[rstest]
    #[tokio::test]
    async fn assistant_singleton_is_idempotent_per_channel(#[future] store: Option<Store>) {
        let Some(store) = store.await else { return };
        let user_id = format!("assistant-singleton-{}", Uuid::new_v4());

        let first = store
            .get_or_create_assistant_conversation(&user_id, "gateway")
            .await
            .expect("first assistant singleton lookup should succeed");
        let second = store
            .get_or_create_assistant_conversation(&user_id, "gateway")
            .await
            .expect("second assistant singleton lookup should succeed");
        let other_channel = store
            .get_or_create_assistant_conversation(&user_id, "telegram")
            .await
            .expect("assistant singleton for another channel should succeed");

        assert_eq!(first, second);
        assert_ne!(first, other_channel);

        cleanup_user(&store, &user_id).await;
    }

    #[rstest]
    #[tokio::test]
    async fn heartbeat_singleton_is_idempotent(#[future] store: Option<Store>) {
        let Some(store) = store.await else { return };
        let user_id = format!("heartbeat-singleton-{}", Uuid::new_v4());

        let first = store
            .get_or_create_heartbeat_conversation(&user_id)
            .await
            .expect("first heartbeat singleton lookup should succeed");
        let second = store
            .get_or_create_heartbeat_conversation(&user_id)
            .await
            .expect("second heartbeat singleton lookup should succeed");

        assert_eq!(first, second);

        let metadata = store
            .get_conversation_metadata(first)
            .await
            .expect("metadata query should succeed")
            .expect("metadata should exist");
        assert_eq!(metadata["thread_type"], "heartbeat");

        cleanup_user(&store, &user_id).await;
    }
}
