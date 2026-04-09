//! Null implementation of NativeWorkspaceStore for NullDatabase.

use uuid::Uuid;

use crate::db::{HybridSearchParams, InsertChunkParams};
use crate::error::WorkspaceError;
use crate::workspace::{
    MemoryChunk as WorkspaceMemoryChunk, MemoryDocument as WorkspaceMemoryDocument,
    SearchResult as WorkspaceSearchResult, WorkspaceEntry as WorkspaceWorkspaceEntry,
};

use super::NullDatabase;

impl crate::db::NativeWorkspaceStore for NullDatabase {
    async fn get_document_by_path(
        &self,
        _user_id: &str,
        _agent_id: Option<Uuid>,
        _path: &str,
    ) -> Result<WorkspaceMemoryDocument, WorkspaceError> {
        Err(NullDatabase::doc_not_found("file"))
    }

    async fn get_document_by_id(
        &self,
        _id: Uuid,
    ) -> Result<WorkspaceMemoryDocument, WorkspaceError> {
        Err(NullDatabase::doc_not_found("id"))
    }

    async fn get_or_create_document_by_path(
        &self,
        _user_id: &str,
        _agent_id: Option<Uuid>,
        _path: &str,
    ) -> Result<WorkspaceMemoryDocument, WorkspaceError> {
        Err(NullDatabase::doc_not_found("file"))
    }

    async fn update_document(&self, _id: Uuid, _content: &str) -> Result<(), WorkspaceError> {
        Ok(())
    }

    async fn delete_document_by_path(
        &self,
        _user_id: &str,
        _agent_id: Option<Uuid>,
        _path: &str,
    ) -> Result<(), WorkspaceError> {
        Ok(())
    }

    async fn list_directory(
        &self,
        _user_id: &str,
        _agent_id: Option<Uuid>,
        _directory: &str,
    ) -> Result<Vec<WorkspaceWorkspaceEntry>, WorkspaceError> {
        Ok(vec![])
    }

    async fn list_all_paths(
        &self,
        _user_id: &str,
        _agent_id: Option<Uuid>,
    ) -> Result<Vec<String>, WorkspaceError> {
        Ok(vec![])
    }

    async fn list_documents(
        &self,
        _user_id: &str,
        _agent_id: Option<Uuid>,
    ) -> Result<Vec<WorkspaceMemoryDocument>, WorkspaceError> {
        Ok(vec![])
    }

    async fn delete_chunks(&self, _document_id: Uuid) -> Result<(), WorkspaceError> {
        Ok(())
    }

    async fn insert_chunk(&self, _params: InsertChunkParams<'_>) -> Result<Uuid, WorkspaceError> {
        Ok(Uuid::new_v4())
    }

    async fn update_chunk_embedding(
        &self,
        _chunk_id: Uuid,
        _embedding: &[f32],
    ) -> Result<(), WorkspaceError> {
        Ok(())
    }

    async fn get_chunks_without_embeddings(
        &self,
        _user_id: &str,
        _agent_id: Option<Uuid>,
        _limit: usize,
    ) -> Result<Vec<WorkspaceMemoryChunk>, WorkspaceError> {
        Ok(vec![])
    }

    async fn hybrid_search(
        &self,
        _params: HybridSearchParams<'_>,
    ) -> Result<Vec<WorkspaceSearchResult>, WorkspaceError> {
        Ok(vec![])
    }
}
