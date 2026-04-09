//! Null implementation of NativeConversationStore for NullDatabase.

use chrono::{DateTime, Utc};
use uuid::Uuid;

use crate::db::EnsureConversationParams;
use crate::error::DatabaseError;
use crate::history::{ConversationMessage, ConversationSummary};

use super::NullDatabase;

impl crate::db::NativeConversationStore for NullDatabase {
    async fn create_conversation(
        &self,
        _channel: &str,
        _user_id: &str,
        _thread_id: Option<&str>,
    ) -> Result<Uuid, DatabaseError> {
        Ok(Uuid::new_v4())
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
        Ok(Uuid::new_v4())
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
        _routine_id: Uuid,
        _routine_name: &str,
        _user_id: &str,
    ) -> Result<Uuid, DatabaseError> {
        Ok(Uuid::new_v4())
    }

    async fn get_or_create_heartbeat_conversation(
        &self,
        _user_id: &str,
    ) -> Result<Uuid, DatabaseError> {
        Ok(Uuid::new_v4())
    }

    async fn get_or_create_assistant_conversation(
        &self,
        _user_id: &str,
        _channel: &str,
    ) -> Result<Uuid, DatabaseError> {
        Ok(Uuid::new_v4())
    }

    async fn create_conversation_with_metadata(
        &self,
        _channel: &str,
        _user_id: &str,
        _metadata: &serde_json::Value,
    ) -> Result<Uuid, DatabaseError> {
        Ok(Uuid::new_v4())
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
