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

impl WorkspaceStorage {
    pub(super) async fn get_document_by_path(
        &self,
        user_id: &str,
        agent_id: Option<Uuid>,
        path: &str,
    ) -> Result<MemoryDocument, WorkspaceError> {
        match self {
            #[cfg(feature = "postgres")]
            Self::Repo(repo) => repo.get_document_by_path(user_id, agent_id, path).await,
            Self::Db(db) => db.get_document_by_path(user_id, agent_id, path).await,
        }
    }

    pub(super) async fn get_document_by_id(
        &self,
        id: Uuid,
    ) -> Result<MemoryDocument, WorkspaceError> {
        match self {
            #[cfg(feature = "postgres")]
            Self::Repo(repo) => repo.get_document_by_id(id).await,
            Self::Db(db) => db.get_document_by_id(id).await,
        }
    }

    pub(super) async fn get_or_create_document_by_path(
        &self,
        user_id: &str,
        agent_id: Option<Uuid>,
        path: &str,
    ) -> Result<MemoryDocument, WorkspaceError> {
        match self {
            #[cfg(feature = "postgres")]
            Self::Repo(repo) => {
                repo.get_or_create_document_by_path(user_id, agent_id, path)
                    .await
            }
            Self::Db(db) => {
                db.get_or_create_document_by_path(user_id, agent_id, path)
                    .await
            }
        }
    }

    pub(super) async fn update_document(
        &self,
        id: Uuid,
        content: &str,
    ) -> Result<(), WorkspaceError> {
        match self {
            #[cfg(feature = "postgres")]
            Self::Repo(repo) => repo.update_document(id, content).await,
            Self::Db(db) => db.update_document(id, content).await,
        }
    }

    pub(super) async fn delete_document_by_path(
        &self,
        user_id: &str,
        agent_id: Option<Uuid>,
        path: &str,
    ) -> Result<(), WorkspaceError> {
        match self {
            #[cfg(feature = "postgres")]
            Self::Repo(repo) => repo.delete_document_by_path(user_id, agent_id, path).await,
            Self::Db(db) => db.delete_document_by_path(user_id, agent_id, path).await,
        }
    }

    pub(super) async fn list_directory(
        &self,
        user_id: &str,
        agent_id: Option<Uuid>,
        directory: &str,
    ) -> Result<Vec<WorkspaceEntry>, WorkspaceError> {
        match self {
            #[cfg(feature = "postgres")]
            Self::Repo(repo) => repo.list_directory(user_id, agent_id, directory).await,
            Self::Db(db) => db.list_directory(user_id, agent_id, directory).await,
        }
    }

    pub(super) async fn list_all_paths(
        &self,
        user_id: &str,
        agent_id: Option<Uuid>,
    ) -> Result<Vec<String>, WorkspaceError> {
        match self {
            #[cfg(feature = "postgres")]
            Self::Repo(repo) => repo.list_all_paths(user_id, agent_id).await,
            Self::Db(db) => db.list_all_paths(user_id, agent_id).await,
        }
    }

    pub(super) async fn delete_chunks(&self, document_id: Uuid) -> Result<(), WorkspaceError> {
        match self {
            #[cfg(feature = "postgres")]
            Self::Repo(repo) => repo.delete_chunks(document_id).await,
            Self::Db(db) => db.delete_chunks(document_id).await,
        }
    }

    pub(super) async fn insert_chunk(
        &self,
        document_id: Uuid,
        chunk_index: u32,
        content: &str,
        embedding: Option<&[f32]>,
    ) -> Result<Uuid, WorkspaceError> {
        match self {
            #[cfg(feature = "postgres")]
            Self::Repo(repo) => {
                repo.insert_chunk(document_id, chunk_index, content, embedding)
                    .await
            }
            Self::Db(db) => {
                db.insert_chunk(InsertChunkParams {
                    document_id,
                    chunk_index,
                    content,
                    embedding,
                })
                .await
            }
        }
    }

    pub(super) async fn update_chunk_embedding(
        &self,
        chunk_id: Uuid,
        embedding: &[f32],
    ) -> Result<(), WorkspaceError> {
        match self {
            #[cfg(feature = "postgres")]
            Self::Repo(repo) => repo.update_chunk_embedding(chunk_id, embedding).await,
            Self::Db(db) => db.update_chunk_embedding(chunk_id, embedding).await,
        }
    }

    pub(super) async fn get_chunks_without_embeddings(
        &self,
        user_id: &str,
        agent_id: Option<Uuid>,
        limit: usize,
    ) -> Result<Vec<MemoryChunk>, WorkspaceError> {
        match self {
            #[cfg(feature = "postgres")]
            Self::Repo(repo) => {
                repo.get_chunks_without_embeddings(user_id, agent_id, limit)
                    .await
            }
            Self::Db(db) => {
                db.get_chunks_without_embeddings(user_id, agent_id, limit)
                    .await
            }
        }
    }

    pub(super) async fn hybrid_search(
        &self,
        user_id: &str,
        agent_id: Option<Uuid>,
        query: &str,
        embedding: Option<&[f32]>,
        config: &SearchConfig,
    ) -> Result<Vec<SearchResult>, WorkspaceError> {
        match self {
            #[cfg(feature = "postgres")]
            Self::Repo(repo) => {
                repo.hybrid_search(user_id, agent_id, query, embedding, config)
                    .await
            }
            Self::Db(db) => {
                db.hybrid_search(HybridSearchParams {
                    user_id,
                    agent_id,
                    query,
                    embedding,
                    config,
                })
                .await
            }
        }
    }
}
