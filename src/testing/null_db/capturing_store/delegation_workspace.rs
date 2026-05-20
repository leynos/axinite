//! Workspace-store delegate implementation for CapturingStore.

use delegate::delegate;
use uuid::Uuid;

use crate::db::{HybridSearchParams, InsertChunkParams};
use crate::error::WorkspaceError;
use crate::workspace::{MemoryChunk, MemoryDocument, SearchResult, WorkspaceEntry};

use super::CapturingStore;

impl crate::db::NativeWorkspaceStore for CapturingStore {
    delegate! {
        to self.inner {
            async fn get_document_by_path(
                &self,
                user_id: &str,
                agent_id: Option<Uuid>,
                path: &str
            ) -> Result<MemoryDocument, WorkspaceError>;
            async fn get_document_by_id(&self, id: Uuid) -> Result<MemoryDocument, WorkspaceError>;
            async fn get_or_create_document_by_path(
                &self,
                user_id: &str,
                agent_id: Option<Uuid>,
                path: &str
            ) -> Result<MemoryDocument, WorkspaceError>;
            async fn update_document(&self, id: Uuid, content: &str) -> Result<(), WorkspaceError>;
            async fn delete_document_by_path(
                &self,
                user_id: &str,
                agent_id: Option<Uuid>,
                path: &str
            ) -> Result<(), WorkspaceError>;
            async fn list_directory(
                &self,
                user_id: &str,
                agent_id: Option<Uuid>,
                directory: &str
            ) -> Result<Vec<WorkspaceEntry>, WorkspaceError>;
            async fn list_all_paths(
                &self,
                user_id: &str,
                agent_id: Option<Uuid>
            ) -> Result<Vec<String>, WorkspaceError>;
            async fn list_documents(
                &self,
                user_id: &str,
                agent_id: Option<Uuid>
            ) -> Result<Vec<MemoryDocument>, WorkspaceError>;
            async fn delete_chunks(&self, document_id: Uuid) -> Result<(), WorkspaceError>;
            async fn insert_chunk(&self, params: InsertChunkParams<'_>) -> Result<Uuid, WorkspaceError>;
            async fn update_chunk_embedding(
                &self,
                chunk_id: Uuid,
                embedding: &[f32]
            ) -> Result<(), WorkspaceError>;
            async fn get_chunks_without_embeddings(
                &self,
                user_id: &str,
                agent_id: Option<Uuid>,
                limit: usize
            ) -> Result<Vec<MemoryChunk>, WorkspaceError>;
            async fn hybrid_search(
                &self,
                params: HybridSearchParams<'_>
            ) -> Result<Vec<SearchResult>, WorkspaceError>;
        }
    }
}
