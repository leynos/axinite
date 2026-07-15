//! Internal storage abstraction dispatching Workspace operations to
//! either the PostgreSQL repository or a generic `Database` backend.

use std::sync::Arc;

use uuid::Uuid;

use crate::db::{HybridSearchParams, InsertChunkParams};
use crate::error::WorkspaceError;

#[cfg(feature = "postgres")]
use super::Repository;
use super::{MemoryChunk, MemoryDocument, SearchConfig, SearchResult, WorkspaceEntry};

/// Internal storage abstraction for Workspace.
///
/// Allows Workspace to work with either a PostgreSQL `Repository` (the original
/// path) or any `Database` trait implementation (e.g. libSQL backend).
pub(super) enum WorkspaceStorage {
    /// PostgreSQL-backed repository (uses connection pool directly).
    #[cfg(feature = "postgres")]
    Repo(Repository),
    /// Generic backend implementing the Database trait.
    Db(Arc<dyn crate::db::Database>),
}

/// Dispatch a method call to whichever backend this storage wraps.
///
/// Both `Repository` and the `Database` trait expose the same method names
/// and signatures, so each wrapper below reduces to a single invocation.
macro_rules! dispatch {
    ($self:expr, $method:ident($($arg:expr),* $(,)?)) => {
        match $self {
            #[cfg(feature = "postgres")]
            Self::Repo(repo) => repo.$method($($arg),*).await,
            Self::Db(db) => db.$method($($arg),*).await,
        }
    };
}

/// Define `pub(super)` async wrappers that forward their arguments verbatim
/// to the backend method of the same name via `dispatch!`.
///
/// Only methods whose wrapper is a pure pass-through belong here; wrappers
/// that assemble parameter structs first are written out manually below.
macro_rules! delegate {
    ($($method:ident($($arg:ident: $ty:ty),* $(,)?) -> $ok:ty;)+) => {
        $(
            pub(super) async fn $method(&self, $($arg: $ty),*) -> Result<$ok, WorkspaceError> {
                dispatch!(self, $method($($arg),*))
            }
        )+
    };
}

impl WorkspaceStorage {
    delegate! {
        get_document_by_path(user_id: &str, agent_id: Option<Uuid>, path: &str) -> MemoryDocument;
        get_document_by_id(id: Uuid) -> MemoryDocument;
        get_or_create_document_by_path(
            user_id: &str,
            agent_id: Option<Uuid>,
            path: &str,
        ) -> MemoryDocument;
        update_document(id: Uuid, content: &str) -> ();
        delete_document_by_path(user_id: &str, agent_id: Option<Uuid>, path: &str) -> ();
        list_directory(
            user_id: &str,
            agent_id: Option<Uuid>,
            directory: &str,
        ) -> Vec<WorkspaceEntry>;
        list_all_paths(user_id: &str, agent_id: Option<Uuid>) -> Vec<String>;
        delete_chunks(document_id: Uuid) -> ();
        update_chunk_embedding(chunk_id: Uuid, embedding: &[f32]) -> ();
        get_chunks_without_embeddings(
            user_id: &str,
            agent_id: Option<Uuid>,
            limit: usize,
        ) -> Vec<MemoryChunk>;
    }

    pub(super) async fn insert_chunk(
        &self,
        document_id: Uuid,
        chunk_index: u32,
        content: &str,
        embedding: Option<&[f32]>,
    ) -> Result<Uuid, WorkspaceError> {
        let params = InsertChunkParams {
            document_id,
            chunk_index,
            content,
            embedding,
        };
        dispatch!(self, insert_chunk(params))
    }

    pub(super) async fn hybrid_search(
        &self,
        user_id: &str,
        agent_id: Option<Uuid>,
        query: &str,
        embedding: Option<&[f32]>,
        config: &SearchConfig,
    ) -> Result<Vec<SearchResult>, WorkspaceError> {
        let params = HybridSearchParams {
            user_id,
            agent_id,
            query,
            embedding,
            config,
        };
        dispatch!(self, hybrid_search(params))
    }
}
