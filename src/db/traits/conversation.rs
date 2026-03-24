//! Conversation persistence traits.
//!
//! Defines the dyn-safe [`ConversationStore`] and its native-async sibling
//! [`NativeConversationStore`] for conversation history and metadata storage.

use core::future::Future;

use chrono::{DateTime, Utc};
use uuid::Uuid;

use crate::db::params::{DbFuture, EnsureConversationParams};
use crate::error::DatabaseError;
use crate::history::{ConversationMessage, ConversationSummary};

/// Object-safe persistence surface for conversation history and metadata.
///
/// This trait provides the dyn-safe boundary for conversation storage
/// operations, enabling trait-object usage (e.g., `Arc<dyn ConversationStore>`).
/// It uses boxed futures ([`DbFuture`]) to maintain object safety.
///
/// Companion trait: [`NativeConversationStore`] provides the same API using
/// native async traits (RPITIT).  A blanket adapter automatically bridges
/// implementations of `NativeConversationStore` to satisfy this trait.
///
/// Thread-safety: All implementations must be `Send + Sync` to support
/// concurrent access.
pub trait ConversationStore: Send + Sync {
    fn create_conversation<'a>(
        &'a self,
        channel: &'a str,
        user_id: &'a str,
        thread_id: Option<&'a str>,
    ) -> DbFuture<'a, Result<Uuid, DatabaseError>>;
    fn touch_conversation<'a>(&'a self, id: Uuid) -> DbFuture<'a, Result<(), DatabaseError>>;
    fn add_conversation_message<'a>(
        &'a self,
        conversation_id: Uuid,
        role: &'a str,
        content: &'a str,
    ) -> DbFuture<'a, Result<Uuid, DatabaseError>>;
    fn ensure_conversation<'a>(
        &'a self,
        params: EnsureConversationParams<'a>,
    ) -> DbFuture<'a, Result<(), DatabaseError>>;
    fn list_conversations_with_preview<'a>(
        &'a self,
        user_id: &'a str,
        channel: &'a str,
        limit: i64,
    ) -> DbFuture<'a, Result<Vec<ConversationSummary>, DatabaseError>>;
    fn list_conversations_all_channels<'a>(
        &'a self,
        user_id: &'a str,
        limit: i64,
    ) -> DbFuture<'a, Result<Vec<ConversationSummary>, DatabaseError>>;
    fn get_or_create_routine_conversation<'a>(
        &'a self,
        routine_id: Uuid,
        routine_name: &'a str,
        user_id: &'a str,
    ) -> DbFuture<'a, Result<Uuid, DatabaseError>>;
    fn get_or_create_heartbeat_conversation<'a>(
        &'a self,
        user_id: &'a str,
    ) -> DbFuture<'a, Result<Uuid, DatabaseError>>;
    fn get_or_create_assistant_conversation<'a>(
        &'a self,
        user_id: &'a str,
        channel: &'a str,
    ) -> DbFuture<'a, Result<Uuid, DatabaseError>>;
    fn create_conversation_with_metadata<'a>(
        &'a self,
        channel: &'a str,
        user_id: &'a str,
        metadata: &'a serde_json::Value,
    ) -> DbFuture<'a, Result<Uuid, DatabaseError>>;
    fn list_conversation_messages_paginated<'a>(
        &'a self,
        conversation_id: Uuid,
        before: Option<DateTime<Utc>>,
        limit: i64,
    ) -> DbFuture<'a, Result<(Vec<ConversationMessage>, bool), DatabaseError>>;
    fn update_conversation_metadata_field<'a>(
        &'a self,
        id: Uuid,
        key: &'a str,
        value: &'a serde_json::Value,
    ) -> DbFuture<'a, Result<(), DatabaseError>>;
    fn get_conversation_metadata<'a>(
        &'a self,
        id: Uuid,
    ) -> DbFuture<'a, Result<Option<serde_json::Value>, DatabaseError>>;
    fn list_conversation_messages<'a>(
        &'a self,
        conversation_id: Uuid,
    ) -> DbFuture<'a, Result<Vec<ConversationMessage>, DatabaseError>>;
    fn conversation_belongs_to_user<'a>(
        &'a self,
        conversation_id: Uuid,
        user_id: &'a str,
    ) -> DbFuture<'a, Result<bool, DatabaseError>>;
}

/// Native async sibling trait for concrete conversation-store implementations.
pub trait NativeConversationStore: Send + Sync {
    fn create_conversation<'a>(
        &'a self,
        channel: &'a str,
        user_id: &'a str,
        thread_id: Option<&'a str>,
    ) -> impl Future<Output = Result<Uuid, DatabaseError>> + Send + 'a;
    fn touch_conversation<'a>(
        &'a self,
        id: Uuid,
    ) -> impl Future<Output = Result<(), DatabaseError>> + Send + 'a;
    fn add_conversation_message<'a>(
        &'a self,
        conversation_id: Uuid,
        role: &'a str,
        content: &'a str,
    ) -> impl Future<Output = Result<Uuid, DatabaseError>> + Send + 'a;
    fn ensure_conversation<'a>(
        &'a self,
        params: EnsureConversationParams<'a>,
    ) -> impl Future<Output = Result<(), DatabaseError>> + Send + 'a;
    fn list_conversations_with_preview<'a>(
        &'a self,
        user_id: &'a str,
        channel: &'a str,
        limit: i64,
    ) -> impl Future<Output = Result<Vec<ConversationSummary>, DatabaseError>> + Send + 'a;
    fn list_conversations_all_channels<'a>(
        &'a self,
        user_id: &'a str,
        limit: i64,
    ) -> impl Future<Output = Result<Vec<ConversationSummary>, DatabaseError>> + Send + 'a;
    fn get_or_create_routine_conversation<'a>(
        &'a self,
        routine_id: Uuid,
        routine_name: &'a str,
        user_id: &'a str,
    ) -> impl Future<Output = Result<Uuid, DatabaseError>> + Send + 'a;
    fn get_or_create_heartbeat_conversation<'a>(
        &'a self,
        user_id: &'a str,
    ) -> impl Future<Output = Result<Uuid, DatabaseError>> + Send + 'a;
    fn get_or_create_assistant_conversation<'a>(
        &'a self,
        user_id: &'a str,
        channel: &'a str,
    ) -> impl Future<Output = Result<Uuid, DatabaseError>> + Send + 'a;
    fn create_conversation_with_metadata<'a>(
        &'a self,
        channel: &'a str,
        user_id: &'a str,
        metadata: &'a serde_json::Value,
    ) -> impl Future<Output = Result<Uuid, DatabaseError>> + Send + 'a;
    fn list_conversation_messages_paginated<'a>(
        &'a self,
        conversation_id: Uuid,
        before: Option<DateTime<Utc>>,
        limit: i64,
    ) -> impl Future<Output = Result<(Vec<ConversationMessage>, bool), DatabaseError>> + Send + 'a;
    fn update_conversation_metadata_field<'a>(
        &'a self,
        id: Uuid,
        key: &'a str,
        value: &'a serde_json::Value,
    ) -> impl Future<Output = Result<(), DatabaseError>> + Send + 'a;
    fn get_conversation_metadata<'a>(
        &'a self,
        id: Uuid,
    ) -> impl Future<Output = Result<Option<serde_json::Value>, DatabaseError>> + Send + 'a;
    fn list_conversation_messages<'a>(
        &'a self,
        conversation_id: Uuid,
    ) -> impl Future<Output = Result<Vec<ConversationMessage>, DatabaseError>> + Send + 'a;
    fn conversation_belongs_to_user<'a>(
        &'a self,
        conversation_id: Uuid,
        user_id: &'a str,
    ) -> impl Future<Output = Result<bool, DatabaseError>> + Send + 'a;
}
