//! Tests for the libSQL workspace-store module split.

use libsql::params;

use super::super::LibSqlBackend;
use super::vector_search::{
    VectorIndexQuery, VectorSearchOutcome, VectorSearchQuery, deserialize_embedding,
    vector_ranked_results,
};
use crate::db::{HybridSearchParams, InsertChunkParams, NativeDatabase, NativeWorkspaceStore};
use crate::workspace::SearchConfig;

/// Assert that `actual` has the same length as `expected` and that every
/// element is within floating-point tolerance.
fn assert_embedding_approx_eq(actual: &[f32], expected: &[f32]) {
    assert_eq!(
        actual.len(),
        expected.len(),
        "embedding length mismatch: got {}, expected {}",
        actual.len(),
        expected.len(),
    );
    for (i, (a, e)) in actual.iter().zip(expected.iter()).enumerate() {
        assert!(
            (a - e).abs() < 0.001,
            "embedding[{i}]: got {a}, expected {e} (tolerance 0.001)",
        );
    }
}

/// Assert that `results` contains exactly one entry whose `document_path`,
/// `fts_rank`, and `vector_rank` match the supplied values.
fn assert_sole_search_result(
    results: &[crate::workspace::SearchResult],
    expected_path: &str,
    expected_fts_rank: Option<u32>,
    expected_vector_rank: Option<u32>,
) {
    assert_eq!(results.len(), 1, "expected exactly one search result");
    let r = &results[0];
    assert_eq!(r.document_path, expected_path, "document_path mismatch");
    assert_eq!(r.fts_rank, expected_fts_rank, "fts_rank mismatch");
    assert_eq!(r.vector_rank, expected_vector_rank, "vector_rank mismatch");
}

/// Create a temp-file-backed [`LibSqlBackend`] with migrations applied,
/// ready for use in unit tests.
async fn setup_backend() -> LibSqlBackend {
    let backend = LibSqlBackend::new_memory()
        .await
        .expect("failed to create in-memory libsql backend");
    backend
        .run_migrations()
        .await
        .expect("failed to run libsql migrations");
    backend
}

/// Assert that `result` is the `DocumentNotFound` error variant.
fn assert_document_not_found<T: std::fmt::Debug>(result: Result<T, crate::error::WorkspaceError>) {
    assert!(
        matches!(
            result,
            Err(crate::error::WorkspaceError::DocumentNotFound { .. })
        ),
        "expected DocumentNotFound, got {:?}",
        result
    );
}

#[test]
fn test_deserialize_embedding_valid() {
    let floats = [1.0f32, 2.0, 3.0];
    let bytes: Vec<u8> = floats.iter().flat_map(|f| f.to_le_bytes()).collect();

    let result = deserialize_embedding(&bytes);

    assert_embedding_approx_eq(&result, &[1.0, 2.0, 3.0]);
}

#[test]
fn test_deserialize_embedding_empty() {
    let result = deserialize_embedding(&[]);
    assert_eq!(result.len(), 0);
}

#[test]
fn test_deserialize_embedding_invalid_length() {
    let result = deserialize_embedding(&[1, 2, 3, 4, 5, 6, 7]);
    assert_eq!(result.len(), 0);
}

#[test]
fn test_deserialize_embedding_single_value() {
    let value = 42.5f32;
    let bytes = value.to_le_bytes();

    let result = deserialize_embedding(&bytes);

    assert_eq!(result.len(), 1);
    assert!((result[0] - 42.5).abs() < 0.001);
}

#[test]
fn test_deserialize_embedding_negative_values() {
    let floats = [-1.5f32, 0.0, 2.75];
    let bytes: Vec<u8> = floats.iter().flat_map(|f| f.to_le_bytes()).collect();

    let result = deserialize_embedding(&bytes);

    assert_embedding_approx_eq(&result, &[-1.5, 0.0, 2.75]);
}

#[tokio::test]
async fn get_chunks_without_embeddings_skips_invalid_chunk_id_uuid() {
    let backend = setup_backend().await;

    let document = backend
        .get_or_create_document_by_path("default", None, "notes/bad-chunk-uuid.md")
        .await
        .expect("failed to create document");

    let conn = backend.connect().await.expect("failed to connect");
    conn.execute(
        "INSERT INTO memory_chunks (id, document_id, chunk_index, content, created_at) \
         VALUES ('not-a-uuid', ?1, 0, 'bad chunk', datetime('now'))",
        params![document.id.to_string()],
    )
    .await
    .expect("failed to insert bad-chunk-id row");

    backend
        .insert_chunk(InsertChunkParams {
            document_id: document.id,
            chunk_index: 1,
            content: "valid chunk",
            embedding: None,
        })
        .await
        .expect("failed to insert valid chunk");

    let chunks = backend
        .get_chunks_without_embeddings("default", None, 10)
        .await
        .expect("get_chunks_without_embeddings must not fail on invalid UUIDs");

    assert_eq!(chunks.len(), 1);
    assert_eq!(chunks[0].content, "valid chunk");
}

#[tokio::test]
async fn get_chunks_without_embeddings_errors_on_negative_chunk_index() {
    let backend = setup_backend().await;

    let document = backend
        .get_or_create_document_by_path("default", None, "notes/neg-idx.md")
        .await
        .expect("failed to create document");

    let conn = backend.connect().await.expect("failed to connect");
    conn.execute(
        "INSERT INTO memory_chunks (id, document_id, chunk_index, content, created_at) \
         VALUES (?1, ?2, -1, 'negative index', datetime('now'))",
        params![uuid::Uuid::new_v4().to_string(), document.id.to_string()],
    )
    .await
    .expect("failed to insert negative-index row");

    let result = backend
        .get_chunks_without_embeddings("default", None, 10)
        .await;

    assert!(
        result.is_err(),
        "get_chunks_without_embeddings must return Err for negative chunk_index"
    );
}

#[tokio::test]
async fn get_document_by_path_returns_not_found_for_missing_document() {
    let backend = setup_backend().await;
    let result = backend
        .get_document_by_path("default", None, "does/not/exist.md")
        .await;
    assert_document_not_found(result);
}

#[tokio::test]
async fn get_document_by_id_returns_not_found_for_unknown_id() {
    let backend = setup_backend().await;
    let result = backend.get_document_by_id(uuid::Uuid::new_v4()).await;
    assert_document_not_found(result);
}

// This test also validates the `collect_vector_index_rows` →
// IndexUnavailable path: the pre-condition assertion confirms
// vector_ranked_results returns IndexUnavailable before the brute-force
// fallback assertions begin.
#[tokio::test]
async fn hybrid_search_uses_brute_force_when_vector_index_is_unavailable() {
    let backend = setup_backend().await;

    let document = backend
        .get_or_create_document_by_path("default", None, "notes/search.md")
        .await
        .expect("failed to create search test document");
    backend
        .update_document(document.id, "semantic search fallback test")
        .await
        .expect("failed to update search test document");
    backend
        .insert_chunk(InsertChunkParams {
            document_id: document.id,
            chunk_index: 0,
            content: "semantic search fallback test",
            embedding: Some(&[1.0, 0.0, 0.0]),
        })
        .await
        .expect("failed to insert search test chunk");

    let conn = backend
        .connect()
        .await
        .expect("failed to open libsql connection for vector precondition");
    let vector_outcome = vector_ranked_results(
        &conn,
        &VectorIndexQuery {
            user_id: "default",
            agent_id: None,
            embedding: &[1.0, 0.0, 0.0],
            limit: 5,
        },
    )
    .await
    .expect("failed to run vector search precondition");
    assert!(
        matches!(vector_outcome, VectorSearchOutcome::IndexUnavailable),
        "Test requires the vector-index-unavailable path before hybrid fallback assertions"
    );

    let results = backend
        .hybrid_search(HybridSearchParams {
            user_id: "default",
            agent_id: None,
            query: "semantic",
            embedding: Some(&[1.0, 0.0, 0.0]),
            config: &SearchConfig::default().with_limit(5),
        })
        .await
        .expect("failed to execute hybrid search");

    assert_sole_search_result(&results, "notes/search.md", Some(1), Some(1));
    assert!(
        results[0].is_hybrid(),
        "brute-force result must be flagged as hybrid"
    );
}

#[tokio::test]
async fn brute_force_vector_search_skips_mismatched_embedding_dimensions() {
    let backend = setup_backend().await;

    let document = backend
        .get_or_create_document_by_path("default", None, "notes/mixed-dim.md")
        .await
        .expect("failed to create mixed-dimension search document");
    backend
        .update_document(document.id, "mixed dimension vector search test")
        .await
        .expect("failed to update mixed-dimension search document");
    backend
        .insert_chunk(InsertChunkParams {
            document_id: document.id,
            chunk_index: 0,
            content: "same-dimension chunk",
            embedding: Some(&[1.0, 0.0, 0.0]),
        })
        .await
        .expect("failed to insert same-dimension chunk");
    backend
        .insert_chunk(InsertChunkParams {
            document_id: document.id,
            chunk_index: 1,
            content: "different-dimension chunk",
            embedding: Some(&[1.0, 0.0]),
        })
        .await
        .expect("failed to insert different-dimension chunk");

    let results = backend
        .brute_force_vector_search(
            VectorSearchQuery {
                user_id: "default",
                agent_id: None,
                embedding: &[1.0, 0.0, 0.0],
            },
            10,
        )
        .await
        .expect("failed to run brute-force vector search");

    assert_eq!(results.len(), 1);
    assert_eq!(results[0].content, "same-dimension chunk");
}

#[tokio::test]
async fn hybrid_search_returns_fts_only_results_without_embedding() {
    let backend = setup_backend().await;

    let document = backend
        .get_or_create_document_by_path("default", None, "notes/fts-only.md")
        .await
        .expect("failed to create FTS-only search document");
    backend
        .insert_chunk(InsertChunkParams {
            document_id: document.id,
            chunk_index: 0,
            content: "keyword only workspace search",
            embedding: None,
        })
        .await
        .expect("failed to insert FTS-only chunk");

    let results = backend
        .hybrid_search(HybridSearchParams {
            user_id: "default",
            agent_id: None,
            query: "keyword",
            embedding: None,
            config: &SearchConfig::default().with_limit(5),
        })
        .await
        .expect("failed to execute FTS-only hybrid search");

    assert_sole_search_result(&results, "notes/fts-only.md", Some(1), None);
}

#[tokio::test]
async fn insert_chunk_and_delete_chunks_round_trip() {
    let backend = setup_backend().await;

    let document = backend
        .get_or_create_document_by_path("default", None, "notes/chunks.md")
        .await
        .expect("failed to create document");

    let chunk_id = backend
        .insert_chunk(InsertChunkParams {
            document_id: document.id,
            chunk_index: 0,
            content: "round-trip chunk",
            embedding: None,
        })
        .await
        .expect("failed to insert chunk");

    let before = backend
        .get_chunks_without_embeddings("default", None, 10)
        .await
        .expect("failed to list chunks before delete");
    assert!(
        before.iter().any(|c| c.id == chunk_id),
        "inserted chunk must appear in get_chunks_without_embeddings"
    );

    backend
        .delete_chunks(document.id)
        .await
        .expect("failed to delete chunks");

    let after = backend
        .get_chunks_without_embeddings("default", None, 10)
        .await
        .expect("failed to list chunks after delete");
    assert!(
        after.iter().all(|c| c.id != chunk_id),
        "deleted chunk must not appear after delete_chunks"
    );
}

#[tokio::test]
async fn update_chunk_embedding_is_reflected_in_chunks_list() {
    let backend = setup_backend().await;

    let document = backend
        .get_or_create_document_by_path("default", None, "notes/embed-update.md")
        .await
        .expect("failed to create document");

    let chunk_id = backend
        .insert_chunk(InsertChunkParams {
            document_id: document.id,
            chunk_index: 0,
            content: "embedding update test",
            embedding: None,
        })
        .await
        .expect("failed to insert chunk");

    let before = backend
        .get_chunks_without_embeddings("default", None, 10)
        .await
        .expect("failed to list chunks before embedding update");
    assert!(
        before.iter().any(|c| c.id == chunk_id),
        "chunk without embedding must appear before update"
    );

    backend
        .update_chunk_embedding(chunk_id, &[0.1, 0.2, 0.3])
        .await
        .expect("failed to update chunk embedding");

    let after = backend
        .get_chunks_without_embeddings("default", None, 10)
        .await
        .expect("failed to list chunks after embedding update");
    assert!(
        after.iter().all(|c| c.id != chunk_id),
        "chunk with embedding must not appear in get_chunks_without_embeddings"
    );
}

#[tokio::test]
async fn get_or_create_document_by_path_is_idempotent() {
    let backend = setup_backend().await;

    let first = backend
        .get_or_create_document_by_path("default", None, "notes/idempotent.md")
        .await
        .expect("failed to create document on first call");
    let second = backend
        .get_or_create_document_by_path("default", None, "notes/idempotent.md")
        .await
        .expect("failed to get document on second call");

    assert_eq!(first.id, second.id, "get_or_create must return the same id");
}

#[tokio::test]
async fn update_document_changes_content() {
    let backend = setup_backend().await;

    let document = backend
        .get_or_create_document_by_path("default", None, "notes/update.md")
        .await
        .expect("failed to create document");
    backend
        .update_document(document.id, "updated content")
        .await
        .expect("failed to update document content");

    let fetched = backend
        .get_document_by_id(document.id)
        .await
        .expect("failed to fetch updated document");
    assert_eq!(
        fetched.content, "updated content",
        "document content must reflect update"
    );
}

#[tokio::test]
async fn delete_document_by_path_removes_document_and_chunks() {
    let backend = setup_backend().await;

    let document = backend
        .get_or_create_document_by_path("default", None, "notes/delete-me.md")
        .await
        .expect("failed to create document");
    backend
        .insert_chunk(InsertChunkParams {
            document_id: document.id,
            chunk_index: 0,
            content: "to be deleted",
            embedding: None,
        })
        .await
        .expect("failed to insert chunk");

    backend
        .delete_document_by_path("default", None, "notes/delete-me.md")
        .await
        .expect("failed to delete document");

    let result = backend
        .get_document_by_path("default", None, "notes/delete-me.md")
        .await;
    assert_document_not_found(result);

    let chunks = backend
        .get_chunks_without_embeddings("default", None, 10)
        .await
        .expect("failed to list chunks after document deletion");
    assert!(
        chunks.iter().all(|c| c.document_id != document.id),
        "chunks belonging to deleted document must be removed"
    );
}

#[tokio::test]
async fn list_all_paths_returns_inserted_document_path() {
    let backend = setup_backend().await;

    backend
        .get_or_create_document_by_path("default", None, "notes/listed.md")
        .await
        .expect("failed to create document");

    let paths = backend
        .list_all_paths("default", None)
        .await
        .expect("failed to list all paths");

    assert!(
        paths.contains(&"notes/listed.md".to_string()),
        "list_all_paths must include inserted document path"
    );
}

#[tokio::test]
async fn list_documents_returns_inserted_document() {
    let backend = setup_backend().await;

    let document = backend
        .get_or_create_document_by_path("default", None, "notes/listed-doc.md")
        .await
        .expect("failed to create document");

    let docs = backend
        .list_documents("default", None)
        .await
        .expect("failed to list documents");

    assert!(
        docs.iter().any(|d| d.id == document.id),
        "list_documents must include inserted document"
    );
}

#[tokio::test]
async fn list_directory_returns_immediate_children_only() {
    let backend = setup_backend().await;

    backend
        .get_or_create_document_by_path("default", None, "notes/dir/child.md")
        .await
        .expect("failed to create child document");
    backend
        .get_or_create_document_by_path("default", None, "notes/dir/sub/deep.md")
        .await
        .expect("failed to create deeply nested document");

    let entries = backend
        .list_directory("default", None, "notes/dir")
        .await
        .expect("failed to list directory");

    assert!(
        entries
            .iter()
            .any(|e| !e.is_directory && e.path.ends_with("child.md")),
        "list_directory must include the direct file child"
    );
    assert!(
        entries
            .iter()
            .any(|e| e.is_directory && e.path.ends_with("sub")),
        "list_directory must include the sub-directory child"
    );
    assert!(
        entries.iter().all(|e| !e.path.ends_with("deep.md")),
        "list_directory must not include deeply nested files"
    );
}
