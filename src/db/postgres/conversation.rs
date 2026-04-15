//! ConversationStore implementation for PostgreSQL backend.

use chrono::{DateTime, Utc};
use uuid::Uuid;

use crate::db::{EnsureConversationParams, NativeConversationStore};
use crate::error::DatabaseError;
use crate::history::{ConversationMessage, ConversationSummary};

use super::PgBackend;

impl NativeConversationStore for PgBackend {
    crate::db::delegate_async! {
        to store;
        async fn create_conversation(&self, channel: &str, user_id: &str, thread_id: Option<&str>) -> Result<Uuid, DatabaseError>;
        async fn touch_conversation(&self, id: Uuid) -> Result<(), DatabaseError>;
        async fn add_conversation_message(&self, conversation_id: Uuid, role: &str, content: &str) -> Result<Uuid, DatabaseError>;
        async fn ensure_conversation(&self, params: EnsureConversationParams<'_>) -> Result<(), DatabaseError>;
        async fn list_conversations_with_preview(&self, user_id: &str, channel: &str, limit: usize) -> Result<Vec<ConversationSummary>, DatabaseError>;
        async fn list_conversations_all_channels(&self, user_id: &str, limit: usize) -> Result<Vec<ConversationSummary>, DatabaseError>;
        async fn get_or_create_routine_conversation(&self, routine_id: Uuid, routine_name: &str, user_id: &str) -> Result<Uuid, DatabaseError>;
        async fn get_or_create_heartbeat_conversation(&self, user_id: &str) -> Result<Uuid, DatabaseError>;
        async fn get_or_create_assistant_conversation(&self, user_id: &str, channel: &str) -> Result<Uuid, DatabaseError>;
        async fn create_conversation_with_metadata(&self, channel: &str, user_id: &str, metadata: &serde_json::Value) -> Result<Uuid, DatabaseError>;
        async fn update_conversation_metadata_field(&self, id: Uuid, key: &str, value: &serde_json::Value) -> Result<(), DatabaseError>;
        async fn get_conversation_metadata(&self, id: Uuid) -> Result<Option<serde_json::Value>, DatabaseError>;
        async fn list_conversation_messages(&self, conversation_id: Uuid) -> Result<Vec<ConversationMessage>, DatabaseError>;
        async fn list_conversation_messages_scoped(&self, conversation_id: Uuid, user_id: &str, channel: &str) -> Result<Vec<ConversationMessage>, DatabaseError>;
        async fn conversation_belongs_to_user(&self, conversation_id: Uuid, user_id: &str) -> Result<bool, DatabaseError>;
    }

    async fn list_conversation_messages_paginated(
        &self,
        conversation_id: Uuid,
        before: Option<(DateTime<Utc>, Uuid)>,
        limit: usize,
    ) -> Result<(Vec<ConversationMessage>, bool), DatabaseError> {
        let limit = i64::try_from(limit).map_err(|_| {
            DatabaseError::Validation("conversation message pagination limit overflow".to_string())
        })?;
        self.store
            .list_conversation_messages_paginated(conversation_id, before, limit)
            .await
    }
}
