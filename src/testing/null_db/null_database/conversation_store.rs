//! Null implementation of NativeConversationStore for NullDatabase.

use chrono::{DateTime, Utc};
use uuid::Uuid;

use crate::db::EnsureConversationParams;
use crate::error::DatabaseError;
use crate::history::{ConversationMessage, ConversationSummary};
use crate::testing::null_db::null_database::{AssistantConvKey, RoutineConvKey};

use super::NullDatabase;

impl crate::db::NativeConversationStore for NullDatabase {
    async fn create_conversation(
        &self,
        _channel: &str,
        _user_id: &str,
        _thread_id: Option<&str>,
    ) -> Result<Uuid, DatabaseError> {
        Ok(self.next_synthetic_uuid())
    }

    async fn touch_conversation(&self, _id: Uuid) -> Result<(), DatabaseError> {
        Ok(())
    }

    async fn add_conversation_message(
        &self,
        _conversation_id: Uuid,
        _role: &str,
        _content: &str,
    ) -> Result<Uuid, DatabaseError> {
        Ok(self.next_synthetic_uuid())
    }

    async fn ensure_conversation(
        &self,
        _params: EnsureConversationParams<'_>,
    ) -> Result<(), DatabaseError> {
        Ok(())
    }

    async fn list_conversations_with_preview(
        &self,
        _user_id: &str,
        _channel: &str,
        _limit: usize,
    ) -> Result<Vec<ConversationSummary>, DatabaseError> {
        Ok(vec![])
    }

    async fn list_conversations_all_channels(
        &self,
        _user_id: &str,
        _limit: usize,
    ) -> Result<Vec<ConversationSummary>, DatabaseError> {
        Ok(vec![])
    }

    async fn get_or_create_routine_conversation(
        &self,
        routine_id: Uuid,
        _routine_name: &str,
        user_id: &str,
    ) -> Result<Uuid, DatabaseError> {
        let key = RoutineConvKey {
            routine_id,
            user_id: user_id.to_string(),
        };
        self.get_or_create_in_cache(&self.routine_conv_cache, key)
            .map_err(|err| DatabaseError::Validation(format!("routine cache poisoned: {err}")))
    }

    async fn get_or_create_heartbeat_conversation(
        &self,
        user_id: &str,
    ) -> Result<Uuid, DatabaseError> {
        self.get_or_create_in_cache(&self.heartbeat_conv_cache, user_id.to_string())
            .map_err(|err| DatabaseError::Validation(format!("heartbeat cache poisoned: {err}")))
    }

    async fn get_or_create_assistant_conversation(
        &self,
        user_id: &str,
        channel: &str,
    ) -> Result<Uuid, DatabaseError> {
        let key = AssistantConvKey {
            user_id: user_id.to_string(),
            channel: channel.to_string(),
        };
        self.get_or_create_in_cache(&self.assistant_conv_cache, key)
            .map_err(|err| DatabaseError::Validation(format!("assistant cache poisoned: {err}")))
    }

    async fn create_conversation_with_metadata(
        &self,
        _channel: &str,
        _user_id: &str,
        _metadata: &serde_json::Value,
    ) -> Result<Uuid, DatabaseError> {
        Ok(self.next_synthetic_uuid())
    }

    async fn update_conversation_metadata_field(
        &self,
        _id: Uuid,
        _key: &str,
        _value: &serde_json::Value,
    ) -> Result<(), DatabaseError> {
        Ok(())
    }

    async fn get_conversation_metadata(
        &self,
        _id: Uuid,
    ) -> Result<Option<serde_json::Value>, DatabaseError> {
        Ok(None)
    }

    async fn list_conversation_messages(
        &self,
        _conversation_id: Uuid,
    ) -> Result<Vec<ConversationMessage>, DatabaseError> {
        Ok(vec![])
    }

    async fn list_conversation_messages_paginated(
        &self,
        _conversation_id: Uuid,
        _before: Option<(DateTime<Utc>, Uuid)>,
        _limit: usize,
    ) -> Result<(Vec<ConversationMessage>, bool), DatabaseError> {
        Ok((vec![], false))
    }

    async fn conversation_belongs_to_user(
        &self,
        _conversation_id: Uuid,
        _user_id: &str,
    ) -> Result<bool, DatabaseError> {
        Ok(false)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::NativeConversationStore;

    #[tokio::test]
    async fn test_get_or_create_routine_conversation_returns_stable_uuid() {
        let db = NullDatabase::new();
        let routine_id = Uuid::new_v4();

        let uuid1 = db
            .get_or_create_routine_conversation(routine_id, "test_routine", "user1")
            .await
            .expect(
                "first get_or_create_routine_conversation for test_routine user1 should succeed",
            );
        let uuid2 = db
            .get_or_create_routine_conversation(routine_id, "test_routine", "user1")
            .await
            .expect(
                "second get_or_create_routine_conversation for test_routine user1 should succeed",
            );

        assert_eq!(uuid1, uuid2, "Same inputs should return same UUID");

        // Different routine_name but same routine_id should return same UUID (singleton semantics)
        let uuid3 = db
            .get_or_create_routine_conversation(routine_id, "different_routine", "user1")
            .await
            .expect("get_or_create_routine_conversation with different routine_name for user1 should succeed");
        assert_eq!(
            uuid1, uuid3,
            "Same routine_id should return same UUID regardless of routine_name"
        );

        let uuid4 = db
            .get_or_create_routine_conversation(Uuid::new_v4(), "test_routine", "user1")
            .await
            .expect("get_or_create_routine_conversation with different routine_id for user1 should succeed");
        assert_ne!(
            uuid1, uuid4,
            "Different routine_id should return different UUID"
        );

        let uuid5 = db
            .get_or_create_routine_conversation(routine_id, "test_routine", "user2")
            .await
            .expect("get_or_create_routine_conversation for user2 should succeed");
        assert_ne!(
            uuid1, uuid5,
            "Different user_id should return different UUID"
        );
    }

    #[tokio::test]
    async fn test_get_or_create_heartbeat_conversation_returns_stable_uuid() {
        let db = NullDatabase::new();

        let uuid1 = db
            .get_or_create_heartbeat_conversation("user1")
            .await
            .expect("first get_or_create_heartbeat_conversation for user1 should succeed");
        let uuid2 = db
            .get_or_create_heartbeat_conversation("user1")
            .await
            .expect("second get_or_create_heartbeat_conversation for user1 should succeed");

        assert_eq!(uuid1, uuid2, "Same user_id should return same UUID");

        // Different user should return different UUID
        let uuid3 = db
            .get_or_create_heartbeat_conversation("user2")
            .await
            .expect("get_or_create_heartbeat_conversation for user2 should succeed");
        assert_ne!(
            uuid1, uuid3,
            "Different user_id should return different UUID"
        );
    }

    #[tokio::test]
    async fn test_get_or_create_assistant_conversation_returns_stable_uuid() {
        let db = NullDatabase::new();

        let uuid1 = db
            .get_or_create_assistant_conversation("user1", "slack")
            .await
            .expect("first get_or_create_assistant_conversation for user1 slack should succeed");
        let uuid2 = db
            .get_or_create_assistant_conversation("user1", "slack")
            .await
            .expect("second get_or_create_assistant_conversation for user1 slack should succeed");

        assert_eq!(uuid1, uuid2, "Same inputs should return same UUID");

        // Different inputs should return different UUIDs
        let uuid3 = db
            .get_or_create_assistant_conversation("user2", "slack")
            .await
            .expect("get_or_create_assistant_conversation for user2 slack should succeed");
        assert_ne!(
            uuid1, uuid3,
            "Different user_id should return different UUID"
        );

        let uuid4 = db
            .get_or_create_assistant_conversation("user1", "discord")
            .await
            .expect("get_or_create_assistant_conversation for user1 discord should succeed");
        assert_ne!(
            uuid1, uuid4,
            "Different channel should return different UUID"
        );
    }
}
