//! Conversation-related `ConversationStore` implementation for `LibSqlBackend`.

mod crud;
mod listing;
mod messages;
mod metadata;
mod singletons;
#[cfg(test)]
mod tests;

use chrono::{DateTime, Utc};
use libsql::params;
use uuid::Uuid;

use super::{LibSqlBackend, fmt_ts, get_i64, get_json, get_opt_text, get_text, get_ts, opt_text};
use crate::db::{EnsureConversationParams, NativeConversationStore};
use crate::error::DatabaseError;
use crate::history::preview_title_from_metadata;
use crate::history::{ConversationMessage, ConversationSummary};

fn parse_uuid(id: String) -> Result<Uuid, DatabaseError> {
    id.parse()
        .map_err(|_| DatabaseError::Serialization("Invalid UUID".to_string()))
}

fn row_to_conversation_summary(row: &libsql::Row) -> Result<ConversationSummary, DatabaseError> {
    let metadata = get_json(row, 3);
    let thread_type = metadata
        .get("thread_type")
        .and_then(|value| value.as_str())
        .map(String::from);
    let sql_title = get_opt_text(row, 6);
    let title = preview_title_from_metadata(&metadata, sql_title);

    Ok(ConversationSummary {
        id: parse_uuid(get_text(row, 0))?,
        started_at: get_ts(row, 1),
        last_activity: get_ts(row, 2),
        message_count: get_i64(row, 5),
        title,
        thread_type,
        channel: get_text(row, 4),
    })
}

impl NativeConversationStore for LibSqlBackend {
    async fn create_conversation(
        &self,
        channel: &str,
        user_id: &str,
        thread_id: Option<&str>,
    ) -> Result<Uuid, DatabaseError> {
        crud::create_conversation(self, channel, user_id, thread_id).await
    }

    async fn touch_conversation(&self, id: Uuid) -> Result<(), DatabaseError> {
        crud::touch_conversation(self, id).await
    }

    async fn add_conversation_message(
        &self,
        conversation_id: Uuid,
        role: &str,
        content: &str,
    ) -> Result<Uuid, DatabaseError> {
        messages::add_conversation_message(self, conversation_id, role, content).await
    }

    async fn ensure_conversation(
        &self,
        params: EnsureConversationParams<'_>,
    ) -> Result<(), DatabaseError> {
        crud::ensure_conversation(self, params).await
    }

    async fn list_conversations_with_preview(
        &self,
        user_id: &str,
        channel: &str,
        limit: usize,
    ) -> Result<Vec<ConversationSummary>, DatabaseError> {
        listing::list_conversations_with_preview(self, user_id, channel, limit).await
    }

    async fn list_conversations_all_channels(
        &self,
        user_id: &str,
        limit: usize,
    ) -> Result<Vec<ConversationSummary>, DatabaseError> {
        listing::list_conversations_all_channels(self, user_id, limit).await
    }

    async fn get_or_create_routine_conversation(
        &self,
        routine_id: Uuid,
        routine_name: &str,
        user_id: &str,
    ) -> Result<Uuid, DatabaseError> {
        singletons::get_or_create_routine_conversation(self, routine_id, routine_name, user_id)
            .await
    }

    async fn get_or_create_heartbeat_conversation(
        &self,
        user_id: &str,
    ) -> Result<Uuid, DatabaseError> {
        singletons::get_or_create_heartbeat_conversation(self, user_id).await
    }

    async fn get_or_create_assistant_conversation(
        &self,
        user_id: &str,
        channel: &str,
    ) -> Result<Uuid, DatabaseError> {
        singletons::get_or_create_assistant_conversation(self, user_id, channel).await
    }

    async fn create_conversation_with_metadata(
        &self,
        channel: &str,
        user_id: &str,
        metadata: &serde_json::Value,
    ) -> Result<Uuid, DatabaseError> {
        crud::create_conversation_with_metadata(self, channel, user_id, metadata).await
    }

    async fn list_conversation_messages_paginated(
        &self,
        conversation_id: Uuid,
        before: Option<(DateTime<Utc>, Uuid)>,
        limit: usize,
    ) -> Result<(Vec<ConversationMessage>, bool), DatabaseError> {
        messages::list_conversation_messages_paginated(self, conversation_id, before, limit).await
    }

    async fn update_conversation_metadata_field(
        &self,
        id: Uuid,
        key: &str,
        value: &serde_json::Value,
    ) -> Result<(), DatabaseError> {
        metadata::update_conversation_metadata_field(self, id, key, value).await
    }

    async fn get_conversation_metadata(
        &self,
        id: Uuid,
    ) -> Result<Option<serde_json::Value>, DatabaseError> {
        metadata::get_conversation_metadata(self, id).await
    }

    async fn list_conversation_messages(
        &self,
        conversation_id: Uuid,
    ) -> Result<Vec<ConversationMessage>, DatabaseError> {
        messages::list_conversation_messages(self, conversation_id).await
    }

    async fn conversation_belongs_to_user(
        &self,
        conversation_id: Uuid,
        user_id: &str,
    ) -> Result<bool, DatabaseError> {
        metadata::conversation_belongs_to_user(self, conversation_id, user_id).await
    }
}
