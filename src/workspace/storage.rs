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

impl WorkspaceStorage {
    pub(super) async fn get_document_by_path(
        &self,
        user_id: &str,
        agent_id: Option<Uuid>,
        path: &str,
    ) -> Result<MemoryDocument, WorkspaceError> {
        dispatch!(self, get_document_by_path(user_id, agent_id, path))
    }

    pub(super) async fn get_document_by_id(
        &self,
        id: Uuid,
    ) -> Result<MemoryDocument, WorkspaceError> {
        dispatch!(self, get_document_by_id(id))
    }

    pub(super) async fn get_or_create_document_by_path(
        &self,
        user_id: &str,
        agent_id: Option<Uuid>,
        path: &str,
    ) -> Result<MemoryDocument, WorkspaceError> {
        dispatch!(
            self,
            get_or_create_document_by_path(user_id, agent_id, path)
        )
    }

    pub(super) async fn update_document(
        &self,
        id: Uuid,
        content: &str,
    ) -> Result<(), WorkspaceError> {
        dispatch!(self, update_document(id, content))
    }

    pub(super) async fn delete_document_by_path(
        &self,
        user_id: &str,
        agent_id: Option<Uuid>,
        path: &str,
    ) -> Result<(), WorkspaceError> {
        dispatch!(self, delete_document_by_path(user_id, agent_id, path))
    }

    pub(super) async fn list_directory(
        &self,
        user_id: &str,
        agent_id: Option<Uuid>,
        directory: &str,
    ) -> Result<Vec<WorkspaceEntry>, WorkspaceError> {
        dispatch!(self, list_directory(user_id, agent_id, directory))
    }

    pub(super) async fn list_all_paths(
        &self,
        user_id: &str,
        agent_id: Option<Uuid>,
    ) -> Result<Vec<String>, WorkspaceError> {
        dispatch!(self, list_all_paths(user_id, agent_id))
    }

    pub(super) async fn delete_chunks(&self, document_id: Uuid) -> Result<(), WorkspaceError> {
        dispatch!(self, delete_chunks(document_id))
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

    pub(super) async fn update_chunk_embedding(
        &self,
        chunk_id: Uuid,
        embedding: &[f32],
    ) -> Result<(), WorkspaceError> {
        dispatch!(self, update_chunk_embedding(chunk_id, embedding))
    }

    pub(super) async fn get_chunks_without_embeddings(
        &self,
        user_id: &str,
        agent_id: Option<Uuid>,
        limit: usize,
    ) -> Result<Vec<MemoryChunk>, WorkspaceError> {
        dispatch!(
            self,
            get_chunks_without_embeddings(user_id, agent_id, limit)
        )
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
