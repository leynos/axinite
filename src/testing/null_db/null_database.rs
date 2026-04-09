//! Null database implementation for tests.
//!
//! All methods return empty defaults (`Ok(None)`, `Ok(vec![])`, etc.).
//! Use this as a baseline for test doubles that need to override only
//! specific methods while delegating the rest to null behaviour.

use crate::error::WorkspaceError;

mod conversation_store;
mod job_store;
mod routine_store;
mod sandbox_store;
mod settings_store;
mod tool_failure_store;
mod workspace_store;

/// A no-op database implementation for testing.
///
/// All methods return empty defaults (`Ok(None)`, `Ok(vec![])`, etc.).
/// Use this as a baseline for test doubles that need to override only
/// specific methods while delegating the rest to null behaviour.
#[derive(Debug, Default)]
pub struct NullDatabase;

impl NullDatabase {
    /// Create a new null database instance.
    pub fn new() -> Self {
        Self
    }

    /// Helper for document-not-found errors in workspace operations.
    pub(super) fn doc_not_found(doc_type: &str) -> WorkspaceError {
        WorkspaceError::DocumentNotFound {
            doc_type: doc_type.to_string(),
            user_id: "test".to_string(),
        }
    }
}

impl crate::db::NativeDatabase for NullDatabase {
    async fn run_migrations(&self) -> Result<(), crate::error::DatabaseError> {
        Ok(())
    }
}
