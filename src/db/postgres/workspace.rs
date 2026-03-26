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
    //! fields to the underlying `Repository` methods in the correct order.

    use super::*;
    use crate::workspace::SearchConfig;
    use mockall::predicate::*;
    use mockall::*;

    // Define a trait that Repository implements (for testing purposes)
    #[automock]
    trait WorkspaceRepository: Send + Sync {
        async fn insert_chunk<'a>(
            &self,
            document_id: Uuid,
            chunk_index: i32,
            content: &'a str,
            embedding: Option<&'a [f32]>,
        ) -> Result<Uuid, WorkspaceError>;

        async fn hybrid_search<'a>(
            &self,
            user_id: &'a str,
            agent_id: Option<Uuid>,
            query: &'a str,
            embedding: Option<&'a [f32]>,
            config: &'a SearchConfig,
        ) -> Result<Vec<SearchResult>, WorkspaceError>;
    }

    // Wrapper to adapt the real Repository to our trait
    struct RepositoryAdapter {
        inner: crate::workspace::Repository,
    }

    impl WorkspaceRepository for RepositoryAdapter {
        async fn insert_chunk(
            &self,
            document_id: Uuid,
            chunk_index: i32,
            content: &str,
            embedding: Option<&[f32]>,
        ) -> Result<Uuid, WorkspaceError> {
            self.inner
                .insert_chunk(document_id, chunk_index, content, embedding)
                .await
        }

        async fn hybrid_search(
            &self,
            user_id: &str,
            agent_id: Option<Uuid>,
            query: &str,
            embedding: Option<&[f32]>,
            config: &SearchConfig,
        ) -> Result<Vec<SearchResult>, WorkspaceError> {
            self.inner
                .hybrid_search(user_id, agent_id, query, embedding, config)
                .await
        }
    }

    // Test-only constructor for PgBackend with a mock repository
    struct MockPgBackend {
        mock_repo: MockWorkspaceRepository,
    }

    impl MockPgBackend {
        fn new(mock_repo: MockWorkspaceRepository) -> Self {
            Self { mock_repo }
        }

        async fn insert_chunk(&self, params: InsertChunkParams<'_>) -> Result<Uuid, WorkspaceError> {
            let InsertChunkParams {
                document_id,
                chunk_index,
                content,
                embedding,
            } = params;
            self.mock_repo
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
            self.mock_repo
                .hybrid_search(user_id, agent_id, query, embedding, config)
                .await
        }
    }

    #[tokio::test]
    async fn test_insert_chunk_forwards_all_parameters() {
        // Arrange: Create test data with distinct values
        let test_doc_id = Uuid::new_v4();
        let test_chunk_index = 42;
        let test_content = "test chunk content with unique text";
        let test_embedding = vec![1.5, 2.7, 3.9, 4.2];
        let expected_chunk_id = Uuid::new_v4();

        // Create a mock that expects exact parameter values
        let mut mock_repo = MockWorkspaceRepository::new();
        mock_repo
            .expect_insert_chunk()
            .with(
                eq(test_doc_id),
                eq(test_chunk_index),
                eq(test_content),
                function(move |emb: &Option<&[f32]>| {
                    emb.map(|e| e.to_vec()) == Some(test_embedding.clone())
                }),
            )
            .times(1)
            .returning(move |_, _, _, _| Ok(expected_chunk_id));

        let backend = MockPgBackend::new(mock_repo);

        // Act: Call insert_chunk with the parameters
        let params = InsertChunkParams {
            document_id: test_doc_id,
            chunk_index: test_chunk_index,
            content: test_content,
            embedding: Some(&test_embedding),
        };

        let result = backend
            .insert_chunk(params)
            .await
            .expect("insert_chunk should succeed");

        // Assert: Verify the result
        assert_eq!(result, expected_chunk_id);
        // The mock expectations are verified on drop
    }

    #[tokio::test]
    async fn test_insert_chunk_forwards_none_embedding() {
        // Arrange: Test with None embedding
        let test_doc_id = Uuid::new_v4();
        let test_chunk_index = 7;
        let test_content = "content without embedding";
        let expected_chunk_id = Uuid::new_v4();

        let mut mock_repo = MockWorkspaceRepository::new();
        mock_repo
            .expect_insert_chunk()
            .with(
                eq(test_doc_id),
                eq(test_chunk_index),
                eq(test_content),
                eq(None::<&[f32]>),
            )
            .times(1)
            .returning(move |_, _, _, _| Ok(expected_chunk_id));

        let backend = MockPgBackend::new(mock_repo);

        // Act
        let params = InsertChunkParams {
            document_id: test_doc_id,
            chunk_index: test_chunk_index,
            content: test_content,
            embedding: None,
        };

        let result = backend
            .insert_chunk(params)
            .await
            .expect("insert_chunk with None embedding should succeed");

        // Assert
        assert_eq!(result, expected_chunk_id);
    }

    #[tokio::test]
    async fn test_hybrid_search_forwards_all_parameters() {
        // Arrange: Create test data with distinct values
        let test_user_id = "test_user_12345";
        let test_agent_id = Some(Uuid::new_v4());
        let test_query = "unique search query text";
        let test_embedding = vec![0.1, 0.2, 0.3, 0.4, 0.5];
        let test_config = SearchConfig {
            limit: 15,
            rrf_k: 75,
            use_fts: true,
            use_vector: true,
            min_score: 0.7,
            pre_fusion_limit: 30,
        };

        let mut mock_repo = MockWorkspaceRepository::new();
        mock_repo
            .expect_hybrid_search()
            .with(
                eq(test_user_id),
                eq(test_agent_id),
                eq(test_query),
                function(move |emb: &Option<&[f32]>| {
                    emb.map(|e| e.to_vec()) == Some(test_embedding.clone())
                }),
                function(move |cfg: &SearchConfig| {
                    cfg.limit == test_config.limit
                        && cfg.rrf_k == test_config.rrf_k
                        && cfg.use_fts == test_config.use_fts
                        && cfg.use_vector == test_config.use_vector
                        && (cfg.min_score - test_config.min_score).abs() < 0.001
                }),
            )
            .times(1)
            .returning(|_, _, _, _, _| Ok(vec![]));

        let backend = MockPgBackend::new(mock_repo);

        // Act: Call hybrid_search with the parameters
        let params = HybridSearchParams {
            user_id: test_user_id,
            agent_id: test_agent_id,
            query: test_query,
            embedding: Some(&test_embedding),
            config: &test_config,
        };

        let result = backend
            .hybrid_search(params)
            .await
            .expect("hybrid_search should succeed");

        // Assert: Verify the result
        assert_eq!(result.len(), 0);
        // The mock expectations are verified on drop
    }

    #[tokio::test]
    async fn test_hybrid_search_forwards_none_agent_and_embedding() {
        // Arrange: Test with None values for agent_id and embedding
        let test_user_id = "another_user";
        let test_query = "query without vector";
        let test_config = SearchConfig {
            limit: 20,
            rrf_k: 60,
            use_fts: true,
            use_vector: false,
            min_score: 0.5,
            pre_fusion_limit: 40,
        };

        let mut mock_repo = MockWorkspaceRepository::new();
        mock_repo
            .expect_hybrid_search()
            .with(
                eq(test_user_id),
                eq(None::<Uuid>),
                eq(test_query),
                eq(None::<&[f32]>),
                function(move |cfg: &SearchConfig| {
                    cfg.limit == test_config.limit
                        && cfg.rrf_k == test_config.rrf_k
                        && cfg.use_fts == test_config.use_fts
                        && !cfg.use_vector
                }),
            )
            .times(1)
            .returning(|_, _, _, _, _| Ok(vec![]));

        let backend = MockPgBackend::new(mock_repo);

        // Act
        let params = HybridSearchParams {
            user_id: test_user_id,
            agent_id: None,
            query: test_query,
            embedding: None,
            config: &test_config,
        };

        let result = backend
            .hybrid_search(params)
            .await
            .expect("hybrid_search with None values should succeed");

        // Assert
        assert_eq!(result.len(), 0);
    }
}
