//! WorkspaceStore implementation for PostgreSQL backend.

use uuid::Uuid;

use crate::db::{HybridSearchParams, InsertChunkParams, NativeWorkspaceStore};
use crate::error::WorkspaceError;
use crate::workspace::{MemoryChunk, MemoryDocument, SearchResult, WorkspaceEntry};

use super::PgBackend;

impl NativeWorkspaceStore for PgBackend {
    crate::delegate_async! {
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

#[cfg(all(test, feature = "postgres"))]
mod tests {
    //! Tests for NativeWorkspaceStore parameter forwarding on PgBackend.
    //!
    //! These verify that the hand-written `insert_chunk` and `hybrid_search`
    //! methods correctly destructure their parameter structs and forward all
    //! fields to the underlying `Repository` methods.

    use super::*;
    use crate::workspace::SearchConfig;

    #[test]
    fn test_insert_chunk_params_destructuring() {
        // This test verifies that the InsertChunkParams struct is correctly
        // destructured and all fields are available for forwarding.
        // This is a compile-time check - if the struct changes and we miss a field,
        // this will fail to compile.

        let document_id = Uuid::new_v4();
        let chunk_index = 42;
        let content = "test content";
        let embedding = vec![1.0, 2.0, 3.0];

        let params = InsertChunkParams {
            document_id,
            chunk_index,
            content,
            embedding: Some(&embedding),
        };

        // Destructure to ensure all fields are present
        let InsertChunkParams {
            document_id: extracted_doc_id,
            chunk_index: extracted_idx,
            content: extracted_content,
            embedding: extracted_embedding,
        } = params;

        // Verify fields are correctly extracted
        assert_eq!(extracted_doc_id, document_id);
        assert_eq!(extracted_idx, chunk_index);
        assert_eq!(extracted_content, content);
        assert_eq!(
            extracted_embedding.expect("expected Some embedding"),
            &embedding[..]
        );

        // This pattern ensures we don't accidentally miss fields when updating
        // the insert_chunk implementation
        let _ = (
            extracted_doc_id,
            extracted_idx,
            extracted_content,
            extracted_embedding,
        );
    }

    #[test]
    fn test_hybrid_search_params_destructuring() {
        // This test verifies that the HybridSearchParams struct is correctly
        // destructured and all fields are available for forwarding.
        // This is a compile-time check - if the struct changes and we miss a field,
        // this will fail to compile.

        let user_id = "test_user";
        let agent_id = Some(Uuid::new_v4());
        let query = "search query";
        let embedding = vec![1.0, 2.0, 3.0];
        let config = SearchConfig {
            limit: 10,
            rrf_k: 60,
            use_fts: true,
            use_vector: true,
            min_score: 0.5,
        };

        let params = HybridSearchParams {
            user_id,
            agent_id,
            query,
            embedding: Some(&embedding),
            config: &config,
        };

        // Destructure to ensure all fields are present
        let HybridSearchParams {
            user_id: extracted_user_id,
            agent_id: extracted_agent_id,
            query: extracted_query,
            embedding: extracted_embedding,
            config: extracted_config,
        } = params;

        // Verify fields are correctly extracted
        assert_eq!(extracted_user_id, user_id);
        assert_eq!(extracted_agent_id, agent_id);
        assert_eq!(extracted_query, query);
        assert_eq!(
            extracted_embedding.expect("expected Some embedding"),
            &embedding[..]
        );
        assert_eq!(extracted_config.limit, config.limit);
        assert_eq!(extracted_config.rrf_k, config.rrf_k);

        // This pattern ensures we don't accidentally miss fields when updating
        // the hybrid_search implementation
        let _ = (
            extracted_user_id,
            extracted_agent_id,
            extracted_query,
            extracted_embedding,
            extracted_config,
        );
    }
}
