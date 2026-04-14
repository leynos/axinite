//! Null database implementation for tests.
//!
//! Most methods return empty defaults (`Ok(None)`, `Ok(vec![])`, etc.), but
//! some return [`WorkspaceError::DocumentNotFound`] for missing documents.
//! UUIDs are generated deterministically via an internal counter (see
//! [`next_synthetic_uuid`](NullDatabase::next_synthetic_uuid)) and cache
//! entries are stable per-key, ensuring reproducible test results.
//! Use this as a baseline for test doubles that need to override only
//! specific methods while delegating the rest to null behavior.

use std::collections::HashMap;
use std::hash::Hash;
use std::sync::Mutex;

use crate::error::WorkspaceError;

mod conversation_store;
mod job_store;
mod routine_store;
mod sandbox_store;
mod settings_store;
mod tool_failure_store;
mod workspace_store;

/// Key for the routine conversation cache.
///
/// Only includes routine_id and user_id to ensure singleton semantics
/// (changing the routine name should not create a new conversation).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(super) struct RoutineConvKey {
    pub routine_id: uuid::Uuid,
    pub user_id: String,
}

/// Key for the assistant conversation cache.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(super) struct AssistantConvKey {
    pub user_id: String,
    pub channel: String,
}

/// A no-op database implementation for testing.
///
/// Most methods return empty defaults (`Ok(None)`, `Ok(vec![])`, etc.), but
/// some return [`WorkspaceError::DocumentNotFound`] for missing documents.
/// UUIDs are generated deterministically via an internal counter (see
/// [`next_synthetic_uuid`](NullDatabase::next_synthetic_uuid)) and cache
/// entries are stable per-key, ensuring reproducible test results.
/// Use this as a baseline for test doubles that need to override only
/// specific methods while delegating the rest to null behavior.
#[derive(Debug, Default)]
pub struct NullDatabase {
    /// Stable UUIDs for routine conversations, keyed by (routine_id, user_id).
    pub(super) routine_conv_cache: Mutex<HashMap<RoutineConvKey, uuid::Uuid>>,
    /// Stable UUIDs for heartbeat conversations, keyed by user_id.
    pub(super) heartbeat_conv_cache: Mutex<HashMap<String, uuid::Uuid>>,
    /// Stable UUIDs for assistant conversations, keyed by (user_id, channel).
    pub(super) assistant_conv_cache: Mutex<HashMap<AssistantConvKey, uuid::Uuid>>,
    /// Counter for deterministic synthetic UUIDs.
    pub(super) uuid_counter: Mutex<u128>,
}

impl NullDatabase {
    /// Create a new null database instance.
    pub fn new() -> Self {
        Self::default()
    }

    /// Helper for document-not-found errors in workspace operations.
    pub(super) fn doc_not_found(doc_type: &str) -> WorkspaceError {
        WorkspaceError::DocumentNotFound {
            doc_type: doc_type.to_string(),
            user_id: "test".to_string(),
        }
    }

    /// Generate a deterministic synthetic UUID based on an internal counter.
    ///
    /// Each call increments the counter and returns a UUID with the counter
    /// value embedded in the UUID bytes. This provides reproducible IDs
    /// for tests that need stable values across multiple calls.
    pub(super) fn next_synthetic_uuid(&self) -> uuid::Uuid {
        // Recover from poisoned mutex to avoid panicking in tests.
        // The counter value is still valid even if a previous holder panicked.
        let mut counter = self
            .uuid_counter
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        *counter += 1;
        // Embed counter in UUID bytes for deterministic generation
        let bytes = counter.to_be_bytes();
        let mut uuid_bytes = [0u8; 16];
        uuid_bytes[0..16].copy_from_slice(&[
            bytes[0], bytes[1], bytes[2], bytes[3], bytes[4], bytes[5], bytes[6], bytes[7],
            bytes[8], bytes[9], bytes[10], bytes[11], bytes[12], bytes[13], bytes[14], bytes[15],
        ]);
        uuid::Uuid::from_bytes(uuid_bytes)
    }

    /// Lock `cache` and return the UUID already stored under `key`,
    /// inserting a fresh synthetic UUID if the entry is absent.
    ///
    /// Recovers from poisoned mutex to avoid panicking in tests.
    pub(super) fn get_or_create_in_cache<K: Eq + Hash>(
        &self,
        cache: &Mutex<HashMap<K, uuid::Uuid>>,
        key: K,
    ) -> uuid::Uuid {
        let mut map = cache
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        *map.entry(key).or_insert_with(|| self.next_synthetic_uuid())
    }
}

impl crate::db::NativeDatabase for NullDatabase {
    async fn persist_terminal_result_and_status(
        &self,
        _params: crate::db::TerminalJobPersistence<'_>,
    ) -> Result<(), crate::error::DatabaseError> {
        Ok(())
    }

    async fn run_migrations(&self) -> Result<(), crate::error::DatabaseError> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn synthetic_uuid_sequence_is_unique_across_many_calls() {
        let db = NullDatabase::new();
        let mut seen = std::collections::HashSet::new();

        for _ in 0..100 {
            let id = db.next_synthetic_uuid();
            assert!(seen.insert(id), "duplicate synthetic UUID: {id}");
        }
    }

    #[test]
    fn cached_ids_are_stable_per_key_and_distinct_across_keys() {
        let db = NullDatabase::new();
        let cache = Mutex::new(HashMap::new());
        let keys = (0..10).map(|idx| format!("key-{idx}")).collect::<Vec<_>>();
        let mut expected = HashMap::new();

        for _ in 0..5 {
            for key in &keys {
                let id = db.get_or_create_in_cache(&cache, key.clone());
                if let Some(existing) = expected.get(key) {
                    assert_eq!(*existing, id, "cache entry for {key} changed");
                } else {
                    expected.insert(key.clone(), id);
                }
            }
        }

        let unique = expected
            .values()
            .copied()
            .collect::<std::collections::HashSet<_>>();
        assert_eq!(unique.len(), keys.len(), "different keys shared a UUID");
    }
}
