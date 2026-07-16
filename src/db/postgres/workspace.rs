//! WorkspaceStore implementation for PostgreSQL backend.

use uuid::Uuid;

use crate::db::{HybridSearchParams, InsertChunkParams, NativeWorkspaceStore};
use crate::error::WorkspaceError;
use crate::workspace::{MemoryChunk, MemoryDocument, SearchResult, WorkspaceEntry};

use super::PgBackend;

impl NativeWorkspaceStore for PgBackend {
    crate::db::delegate_async! {
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
        self.repo.insert_chunk(params).await
    }

    async fn hybrid_search(
        &self,
        params: HybridSearchParams<'_>,
    ) -> Result<Vec<SearchResult>, WorkspaceError> {
        self.repo.hybrid_search(params).await
    }
}

#[cfg(all(test, feature = "postgres"))]
mod tests {
    //! Behavioural tests for NativeWorkspaceStore on PgBackend.
    //!
    //! These exercise the real Postgres backend to verify that
    //! `insert_chunk` and `hybrid_search` correctly persist and
    //! retrieve data through the full delegation chain.

    use super::*;
    use crate::error::DatabaseError;
    use crate::testing::postgres::try_test_pg_db;
    use crate::workspace::SearchConfig;
    use anyhow::Context as _;
    use rstest::{fixture, rstest};

    #[fixture]
    async fn db() -> Result<Option<PgBackend>, DatabaseError> {
        try_test_pg_db().await
    }

    /// Create a document owned by a unique user so chunks can reference it.
    async fn setup_document(db: &PgBackend) -> anyhow::Result<(String, MemoryDocument)> {
        setup_document_for_agent(db, None).await
    }

    /// Create a document owned by a unique user and optional agent.
    async fn setup_document_for_agent(
        db: &PgBackend,
        agent_id: Option<Uuid>,
    ) -> anyhow::Result<(String, MemoryDocument)> {
        let user_id = format!("ws-test-{}", Uuid::new_v4());
        let path = format!("/test/{}.md", Uuid::new_v4());
        let doc = db
            .get_or_create_document_by_path(&user_id, agent_id, &path)
            .await
            .context("get_or_create_document_by_path should succeed")?;
        Ok((user_id, doc))
    }

    #[rstest]
    #[tokio::test]
    async fn test_insert_chunk_persists_with_embedding(
        #[future] db: Result<Option<PgBackend>, DatabaseError>,
    ) {
        let Some(db) = db.await.expect("unexpected Postgres test setup error") else {
            return;
        };
        let (user_id, doc) = setup_document(&db)
            .await
            .expect("setup_document should succeed");

        let embedding = vec![1.5, 2.7, 3.9, 4.2];
        let params = InsertChunkParams {
            document_id: doc.id,
            chunk_index: 0,
            content: "chunk with embedding",
            embedding: Some(&embedding),
        };

        let chunk_id = db
            .insert_chunk(params)
            .await
            .expect("insert_chunk should succeed");

        // Verify the chunk was persisted by deleting and re-checking
        // (delete_chunks removes all chunks for a document)
        assert_ne!(chunk_id, Uuid::nil());

        let missing = db
            .get_chunks_without_embeddings(&user_id, None, 100)
            .await
            .expect("get_chunks_without_embeddings should succeed");

        assert!(
            !missing.iter().any(|chunk| chunk.id == chunk_id),
            "chunk with embedding should not appear in \
             get_chunks_without_embeddings"
        );
    }

    #[rstest]
    #[tokio::test]
    async fn test_insert_chunk_persists_without_embedding(
        #[future] db: Result<Option<PgBackend>, DatabaseError>,
    ) {
        let Some(db) = db.await.expect("unexpected Postgres test setup error") else {
            return;
        };
        let (user_id, doc) = setup_document(&db)
            .await
            .expect("setup_document should succeed");

        let params = InsertChunkParams {
            document_id: doc.id,
            chunk_index: 0,
            content: "chunk without embedding",
            embedding: None,
        };

        let chunk_id = db
            .insert_chunk(params)
            .await
            .expect("insert_chunk with None embedding should succeed");

        assert_ne!(chunk_id, Uuid::nil());

        // The chunk should appear in the without-embeddings list
        let missing = db
            .get_chunks_without_embeddings(&user_id, None, 100)
            .await
            .expect("get_chunks_without_embeddings should succeed");

        assert!(
            missing.iter().any(|c| c.id == chunk_id),
            "chunk without embedding should appear in \
             get_chunks_without_embeddings"
        );
    }

    #[rstest]
    #[tokio::test]
    async fn test_insert_chunk_fields_round_trip(
        #[future] db: Result<Option<PgBackend>, DatabaseError>,
    ) {
        let Some(db) = db.await.expect("unexpected Postgres test setup error") else {
            return;
        };
        let (user_id, doc) = setup_document(&db)
            .await
            .expect("setup_document should succeed");

        let content = "round-trip content verification";
        let params = InsertChunkParams {
            document_id: doc.id,
            chunk_index: 3,
            content,
            embedding: None,
        };

        let chunk_id = db
            .insert_chunk(params)
            .await
            .expect("insert_chunk should succeed");

        // Retrieve via get_chunks_without_embeddings and verify fields
        let chunks = db
            .get_chunks_without_embeddings(&user_id, None, 100)
            .await
            .expect("get_chunks_without_embeddings should succeed");

        let chunk = chunks
            .iter()
            .find(|c| c.id == chunk_id)
            .expect("inserted chunk should be retrievable");

        assert_eq!(chunk.document_id, doc.id);
        assert_eq!(chunk.chunk_index, 3);
        assert_eq!(chunk.content, content);
        assert!(chunk.embedding.is_none());
    }

    #[rstest]
    #[tokio::test]
    async fn test_hybrid_search_returns_inserted_chunk(
        #[future] db: Result<Option<PgBackend>, DatabaseError>,
    ) {
        let Some(db) = db.await.expect("unexpected Postgres test setup error") else {
            return;
        };
        let (user_id, doc) = setup_document(&db)
            .await
            .expect("setup_document should succeed");

        // Insert a chunk with searchable content
        let content = "the quick brown fox jumps over the lazy dog";
        let params = InsertChunkParams {
            document_id: doc.id,
            chunk_index: 0,
            content,
            embedding: None,
        };

        db.insert_chunk(params)
            .await
            .expect("insert_chunk should succeed");

        // Search using FTS only (no embedding)
        let config = SearchConfig::default().fts_only();
        let search_params = HybridSearchParams {
            user_id: &user_id,
            agent_id: None,
            query: "quick brown fox",
            embedding: None,
            config: &config,
        };

        let results = db
            .hybrid_search(search_params)
            .await
            .expect("hybrid_search should succeed");

        // The inserted chunk should appear in the results
        assert!(
            results.iter().any(|r| r.document_id == doc.id),
            "hybrid_search should find the inserted chunk"
        );
    }

    #[rstest]
    #[tokio::test]
    async fn test_hybrid_search_respects_user_isolation(
        #[future] db: Result<Option<PgBackend>, DatabaseError>,
    ) {
        let Some(db) = db.await.expect("unexpected Postgres test setup error") else {
            return;
        };
        let (_user_id, doc) = setup_document(&db)
            .await
            .expect("setup_document should succeed");
        let other_user = format!("ws-other-{}", Uuid::new_v4());

        // Insert a chunk under user_id
        let content = "classified workspace material alpha bravo";
        let params = InsertChunkParams {
            document_id: doc.id,
            chunk_index: 0,
            content,
            embedding: None,
        };

        db.insert_chunk(params)
            .await
            .expect("insert_chunk should succeed");

        // Search as a different user — should not find the chunk
        let config = SearchConfig::default().fts_only();
        let search_params = HybridSearchParams {
            user_id: &other_user,
            agent_id: None,
            query: "classified workspace material",
            embedding: None,
            config: &config,
        };

        let results = db
            .hybrid_search(search_params)
            .await
            .expect("hybrid_search should succeed");

        assert!(
            !results.iter().any(|r| r.document_id == doc.id),
            "hybrid_search should not return chunks belonging \
             to a different user"
        );
    }

    #[rstest]
    #[tokio::test]
    async fn test_hybrid_search_respects_agent_scope(
        #[future] db: Result<Option<PgBackend>, DatabaseError>,
    ) {
        let Some(db) = db.await.expect("unexpected Postgres test setup error") else {
            return;
        };
        let agent_id = Uuid::new_v4();
        let (user_id, doc) = setup_document_for_agent(&db, Some(agent_id))
            .await
            .expect("setup_document_for_agent should succeed");

        db.insert_chunk(InsertChunkParams {
            document_id: doc.id,
            chunk_index: 0,
            content: "agent scoped workspace material",
            embedding: None,
        })
        .await
        .expect("insert_chunk should succeed");

        let config = SearchConfig::default().fts_only();
        let scoped_results = db
            .hybrid_search(HybridSearchParams {
                user_id: &user_id,
                agent_id: Some(agent_id),
                query: "agent scoped workspace",
                embedding: None,
                config: &config,
            })
            .await
            .expect("agent-scoped hybrid_search should succeed");

        assert!(
            scoped_results
                .iter()
                .any(|result| result.document_id == doc.id),
            "agent-scoped hybrid_search should find the chunk"
        );

        let unscoped_results = db
            .hybrid_search(HybridSearchParams {
                user_id: &user_id,
                agent_id: None,
                query: "agent scoped workspace",
                embedding: None,
                config: &config,
            })
            .await
            .expect("unscoped hybrid_search should succeed");

        assert!(
            !unscoped_results
                .iter()
                .any(|result| result.document_id == doc.id),
            "unscoped hybrid_search should not return agent-scoped chunks"
        );
    }
}
