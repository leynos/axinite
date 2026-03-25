//! WorkspaceStore implementation for PostgreSQL backend.

use uuid::Uuid;

use crate::db::{HybridSearchParams, InsertChunkParams, NativeWorkspaceStore};
use crate::error::WorkspaceError;
use crate::workspace::{MemoryChunk, MemoryDocument, SearchResult, WorkspaceEntry};

use super::PgBackend;

impl NativeWorkspaceStore for PgBackend {
    delegate_async! {
        to repo;
        async fn get_document_by_path(&self, user_id: &str, agent_id: Option<Uuid>, path: &str) -> Result<MemoryDocument, WorkspaceError>;
        async fn get_document_by_id(&self, id: Uuid) -> Result<MemoryDocument, WorkspaceError>;
        async fn get_or_create_document_by_path(&self, user_id: &str, agent_id: Option<Uuid>, path: &str) -> Result<MemoryDocument, WorkspaceError>;
        async fn update_document(&self, id: Uuid, content: &str) -> Result<(), WorkspaceError>;
        async fn delete_document_by_path(&self, user_id: &str, agent_id: Option<Uuid>, path: &str) -> Result<(), WorkspaceError>;
        async fn list_directory(&self, user_id: &str, agent_id: Option<Uuid>, directory: &str) -> Result<Vec<WorkspaceEntry>, WorkspaceError>;
        async fn list_all_paths(&self, user_id: &str, agent_id: Option<Uuid>) -> Result<Vec<String>, WorkspaceError>;
        async fn list_documents(&self, user_id: &str, agent_id: Option<Uuid>) -> Result<Vec<MemoryDocument>, WorkspaceError>;
        async fn delete_chunks(&self, document_id: Uuid) -> Result<(), WorkspaceError>;
        async fn update_chunk_embedding(&self, chunk_id: Uuid, embedding: &[f32]) -> Result<(), WorkspaceError>;
        async fn get_chunks_without_embeddings(&self, user_id: &str, agent_id: Option<Uuid>, limit: usize) -> Result<Vec<MemoryChunk>, WorkspaceError>;
    }

    async fn insert_chunk(&self, params: InsertChunkParams<'_>) -> Result<Uuid, WorkspaceError> {
        let InsertChunkParams {
            document_id,
            chunk_index,
            content,
            embedding,
        } = params;
        self.repo
            .insert_chunk(document_id, chunk_index, content, embedding)
            .await
    }

    async fn hybrid_search(
        &self,
        params: HybridSearchParams<'_>,
    ) -> Result<Vec<SearchResult>, WorkspaceError> {
        let HybridSearchParams {
            user_id,
            agent_id,
            query,
            embedding,
            config,
        } = params;
        self.repo
            .hybrid_search(user_id, agent_id, query, embedding, config)
            .await
    }
}
