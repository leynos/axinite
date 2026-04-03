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
