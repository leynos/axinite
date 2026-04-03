//! Conversation persistence traits.
//!
//! Defines the dyn-safe [`ConversationStore`] and its native-async sibling
//! [`NativeConversationStore`] for conversation history and metadata storage.

use core::future::Future;

use chrono::{DateTime, Utc};
use uuid::Uuid;

use crate::db::params::DbFuture;
use crate::error::DatabaseError;
use crate::history::{ConversationMessage, ConversationSummary};

/// Parameters for `ensure_conversation`.
pub struct EnsureConversationParams<'a> {
    /// Stable conversation UUID to create or re-confirm.
    pub id: Uuid,
    /// Channel identifier that owns the conversation, such as `"web"` or `"routine"`.
    pub channel: &'a str,
    /// Owning user identifier string for access control and scoping.
    pub user_id: &'a str,
    /// Optional thread identifier for threaded channels; `None` for channels
    /// that do not expose a separate thread key.
    pub thread_id: Option<&'a str>,
}

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
    /// Create a conversation for the given channel/user/thread tuple.
    ///
    /// Returns the persisted conversation ID or `DatabaseError` if the write
    /// fails.
    fn create_conversation<'a>(
        &'a self,
        channel: &'a str,
        user_id: &'a str,
        thread_id: Option<&'a str>,
    ) -> DbFuture<'a, Result<Uuid, DatabaseError>>;
    /// Update `last_activity` for an existing conversation.
    ///
    /// Returns `DatabaseError` when the timestamp update cannot be persisted.
    fn touch_conversation<'a>(&'a self, id: Uuid) -> DbFuture<'a, Result<(), DatabaseError>>;
    /// Append a message to a conversation and return the new message ID.
    ///
    /// Implementations should persist both the message and any associated
    /// activity updates atomically when possible.
    fn add_conversation_message<'a>(
        &'a self,
        conversation_id: Uuid,
        role: &'a str,
        content: &'a str,
    ) -> DbFuture<'a, Result<Uuid, DatabaseError>>;
    /// Ensure a conversation with the supplied identity fields exists.
    ///
    /// This is idempotent and returns `DatabaseError` if the upsert fails.
    fn ensure_conversation<'a>(
        &'a self,
        params: EnsureConversationParams<'a>,
    ) -> DbFuture<'a, Result<(), DatabaseError>>;
    /// List recent conversations for one user/channel with preview metadata.
    ///
    /// `limit` bounds the number of summaries returned; persistence and decode
    /// failures surface as `DatabaseError`.
    fn list_conversations_with_preview<'a>(
        &'a self,
        user_id: &'a str,
        channel: &'a str,
        limit: usize,
    ) -> DbFuture<'a, Result<Vec<ConversationSummary>, DatabaseError>>;
    /// List recent conversations for one user across all channels.
    ///
    /// Results are typically ordered by most recent activity and capped by
    /// `limit`.
    fn list_conversations_all_channels<'a>(
        &'a self,
        user_id: &'a str,
        limit: usize,
    ) -> DbFuture<'a, Result<Vec<ConversationSummary>, DatabaseError>>;
    /// Get or create the routine conversation owned by `routine_id`.
    ///
    /// Returns the existing or newly created conversation ID.
    fn get_or_create_routine_conversation<'a>(
        &'a self,
        routine_id: Uuid,
        routine_name: &'a str,
        user_id: &'a str,
    ) -> DbFuture<'a, Result<Uuid, DatabaseError>>;
    /// Get or create the singleton heartbeat conversation for `user_id`.
    ///
    /// Returns the stable conversation ID or `DatabaseError` on persistence
    /// failure.
    fn get_or_create_heartbeat_conversation<'a>(
        &'a self,
        user_id: &'a str,
    ) -> DbFuture<'a, Result<Uuid, DatabaseError>>;
    /// Get or create the singleton assistant conversation for a user/channel.
    ///
    /// Implementations should preserve singleton semantics for the supplied
    /// pair.
    fn get_or_create_assistant_conversation<'a>(
        &'a self,
        user_id: &'a str,
        channel: &'a str,
    ) -> DbFuture<'a, Result<Uuid, DatabaseError>>;
    /// Create a conversation row with caller-supplied metadata.
    ///
    /// The returned UUID identifies the new conversation if the insert
    /// succeeds.
    fn create_conversation_with_metadata<'a>(
        &'a self,
        channel: &'a str,
        user_id: &'a str,
        metadata: &'a serde_json::Value,
    ) -> DbFuture<'a, Result<Uuid, DatabaseError>>;
    /// Page backward through conversation messages.
    ///
    /// `before` is an exclusive cursor; the boolean indicates whether more
    /// messages remain before the returned window.
    fn list_conversation_messages_paginated<'a>(
        &'a self,
        conversation_id: Uuid,
        before: Option<(DateTime<Utc>, Uuid)>,
        limit: usize,
    ) -> DbFuture<'a, Result<(Vec<ConversationMessage>, bool), DatabaseError>>;
    /// Merge one metadata field into the stored conversation metadata object.
    ///
    /// Returns `DatabaseError` if the patch cannot be serialized or persisted.
    fn update_conversation_metadata_field<'a>(
        &'a self,
        id: Uuid,
        key: &'a str,
        value: &'a serde_json::Value,
    ) -> DbFuture<'a, Result<(), DatabaseError>>;
    /// Load conversation metadata for `id`.
    ///
    /// Returns `Ok(None)` when the row is missing or metadata is `NULL`.
    fn get_conversation_metadata<'a>(
        &'a self,
        id: Uuid,
    ) -> DbFuture<'a, Result<Option<serde_json::Value>, DatabaseError>>;
    /// List all messages for a conversation in chronological order.
    ///
    /// Implementations return owned message records and surface storage issues
    /// as `DatabaseError`.
    fn list_conversation_messages<'a>(
        &'a self,
        conversation_id: Uuid,
    ) -> DbFuture<'a, Result<Vec<ConversationMessage>, DatabaseError>>;
    /// Check whether the conversation is owned by `user_id`.
    ///
    /// Returns `Ok(false)` for missing or foreign rows.
    fn conversation_belongs_to_user<'a>(
        &'a self,
        conversation_id: Uuid,
        user_id: &'a str,
    ) -> DbFuture<'a, Result<bool, DatabaseError>>;
}

/// Native async sibling trait for concrete conversation-store implementations.
pub trait NativeConversationStore: Send + Sync {
    /// Create a conversation for the given channel/user/thread tuple.
    fn create_conversation<'a>(
        &'a self,
        channel: &'a str,
        user_id: &'a str,
        thread_id: Option<&'a str>,
    ) -> impl Future<Output = Result<Uuid, DatabaseError>> + Send + 'a;
    /// Update `last_activity` for an existing conversation.
    fn touch_conversation<'a>(
        &'a self,
        id: Uuid,
    ) -> impl Future<Output = Result<(), DatabaseError>> + Send + 'a;
    /// Append a message to a conversation and return the new message ID.
    fn add_conversation_message<'a>(
        &'a self,
        conversation_id: Uuid,
        role: &'a str,
        content: &'a str,
    ) -> impl Future<Output = Result<Uuid, DatabaseError>> + Send + 'a;
    /// Ensure a conversation with the supplied identity fields exists.
    fn ensure_conversation<'a>(
        &'a self,
        params: EnsureConversationParams<'a>,
    ) -> impl Future<Output = Result<(), DatabaseError>> + Send + 'a;
    /// List recent conversations for one user/channel with preview metadata.
    fn list_conversations_with_preview<'a>(
        &'a self,
        user_id: &'a str,
        channel: &'a str,
        limit: usize,
    ) -> impl Future<Output = Result<Vec<ConversationSummary>, DatabaseError>> + Send + 'a;
    /// List recent conversations for one user across all channels.
    fn list_conversations_all_channels<'a>(
        &'a self,
        user_id: &'a str,
        limit: usize,
    ) -> impl Future<Output = Result<Vec<ConversationSummary>, DatabaseError>> + Send + 'a;
    /// Get or create the routine conversation owned by `routine_id`.
    fn get_or_create_routine_conversation<'a>(
        &'a self,
        routine_id: Uuid,
        routine_name: &'a str,
        user_id: &'a str,
    ) -> impl Future<Output = Result<Uuid, DatabaseError>> + Send + 'a;
    /// Get or create the singleton heartbeat conversation for `user_id`.
    fn get_or_create_heartbeat_conversation<'a>(
        &'a self,
        user_id: &'a str,
    ) -> impl Future<Output = Result<Uuid, DatabaseError>> + Send + 'a;
    /// Get or create the singleton assistant conversation for a user/channel.
    fn get_or_create_assistant_conversation<'a>(
        &'a self,
        user_id: &'a str,
        channel: &'a str,
    ) -> impl Future<Output = Result<Uuid, DatabaseError>> + Send + 'a;
    /// Create a conversation row with caller-supplied metadata.
    fn create_conversation_with_metadata<'a>(
        &'a self,
        channel: &'a str,
        user_id: &'a str,
        metadata: &'a serde_json::Value,
    ) -> impl Future<Output = Result<Uuid, DatabaseError>> + Send + 'a;
    /// Page backward through conversation messages.
    fn list_conversation_messages_paginated<'a>(
        &'a self,
        conversation_id: Uuid,
        before: Option<(DateTime<Utc>, Uuid)>,
        limit: usize,
    ) -> impl Future<Output = Result<(Vec<ConversationMessage>, bool), DatabaseError>> + Send + 'a;
    /// Merge one metadata field into the stored conversation metadata object.
    fn update_conversation_metadata_field<'a>(
        &'a self,
        id: Uuid,
        key: &'a str,
        value: &'a serde_json::Value,
    ) -> impl Future<Output = Result<(), DatabaseError>> + Send + 'a;
    /// Load conversation metadata for `id`.
    fn get_conversation_metadata<'a>(
        &'a self,
        id: Uuid,
    ) -> impl Future<Output = Result<Option<serde_json::Value>, DatabaseError>> + Send + 'a;
    /// List all messages for a conversation in chronological order.
    fn list_conversation_messages<'a>(
        &'a self,
        conversation_id: Uuid,
    ) -> impl Future<Output = Result<Vec<ConversationMessage>, DatabaseError>> + Send + 'a;
    /// Check whether the conversation is owned by `user_id`.
    fn conversation_belongs_to_user<'a>(
        &'a self,
        conversation_id: Uuid,
        user_id: &'a str,
    ) -> impl Future<Output = Result<bool, DatabaseError>> + Send + 'a;
}
