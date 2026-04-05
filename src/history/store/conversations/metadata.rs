//! Conversation metadata read/write helpers.

use uuid::Uuid;

use super::Store;
use crate::error::DatabaseError;

#[cfg(feature = "postgres")]
impl Store {
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
        Ok(row.and_then(|r| r.get::<_, Option<serde_json::Value>>(0)))
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
        let backend = try_test_pg_db()
            .await
            .expect("unexpected Postgres test setup error")?;
        Some(Store::from_pool(backend.pool()))
    }

    async fn seed_conversation(store: &Store) -> Uuid {
        store
            .create_conversation("gateway", "metadata-user", None)
            .await
            .expect("conversation should be created")
    }

    async fn cleanup(store: &Store, id: Uuid) {
        let conn = store.conn().await.expect("connection should be available");
        conn.execute("DELETE FROM conversations WHERE id = $1", &[&id])
            .await
            .expect("conversation should be deleted");
    }

    #[rstest]
    #[tokio::test]
    async fn conversation_metadata_round_trips(#[future] store: Option<Store>) {
        let Some(store) = store.await else { return };
        let conversation_id = seed_conversation(&store).await;

        store
            .update_conversation_metadata_field(
                conversation_id,
                "thread_type",
                &serde_json::json!("assistant"),
            )
            .await
            .expect("first metadata update should succeed");
        store
            .update_conversation_metadata_field(
                conversation_id,
                "routine_name",
                &serde_json::json!("daily-standup"),
            )
            .await
            .expect("second metadata update should succeed");

        let metadata = store
            .get_conversation_metadata(conversation_id)
            .await
            .expect("metadata query should succeed")
            .expect("metadata should exist");
        assert_eq!(metadata["thread_type"], "assistant");
        assert_eq!(metadata["routine_name"], "daily-standup");

        cleanup(&store, conversation_id).await;
    }
}
