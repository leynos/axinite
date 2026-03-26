//! Persistence helpers for thread operations.
//!
//! Contains utilities for building database parameters and managing conversation persistence.

use crate::db::EnsureConversationParams;
use uuid::Uuid;

/// Helper to build EnsureConversationParams for gateway conversations.
///
/// Gateway conversations use channel="gateway", id=thread_id, and thread_id=None.
pub(super) fn gateway_conversation_params(
    thread_id: Uuid,
    user_id: &str,
) -> EnsureConversationParams<'_> {
    EnsureConversationParams {
        id: thread_id,
        channel: "gateway",
        user_id,
        thread_id: None,
    }
}
