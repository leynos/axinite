//! Tests for the libSQL workspace-store module split.

use super::super::LibSqlBackend;
use super::vector_search::{
    VectorIndexQuery, VectorSearchOutcome, VectorSearchQuery, deserialize_embedding,
    vector_ranked_results,
};
use crate::db::{HybridSearchParams, InsertChunkParams, NativeDatabase, NativeWorkspaceStore};
use crate::workspace::SearchConfig;

#[test]
fn test_deserialize_embedding_valid() {
    let floats = [1.0f32, 2.0, 3.0];
    let bytes: Vec<u8> = floats.iter().flat_map(|f| f.to_le_bytes()).collect();

    let result = deserialize_embedding(&bytes);

    assert_eq!(result.len(), 3);
    assert!((result[0] - 1.0).abs() < 0.001);
    assert!((result[1] - 2.0).abs() < 0.001);
    assert!((result[2] - 3.0).abs() < 0.001);
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

    assert_eq!(result.len(), 3);
    assert!((result[0] - (-1.5)).abs() < 0.001);
    assert!((result[1] - 0.0).abs() < 0.001);
    assert!((result[2] - 2.75).abs() < 0.001);
}

#[tokio::test]
async fn hybrid_search_uses_brute_force_when_vector_index_is_unavailable() {
    let backend = LibSqlBackend::new_memory()
        .await
        .expect("failed to create in-memory libsql backend");
    backend
        .run_migrations()
        .await
        .expect("failed to run libsql migrations");

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

    assert_eq!(results.len(), 1);
    assert_eq!(results[0].document_path, "notes/search.md");
    assert_eq!(results[0].fts_rank, Some(1));
    assert_eq!(results[0].vector_rank, Some(1));
    assert!(results[0].is_hybrid());
}

#[tokio::test]
async fn brute_force_vector_search_skips_mismatched_embedding_dimensions() {
    let backend = LibSqlBackend::new_memory()
        .await
        .expect("failed to create in-memory libsql backend");
    backend
        .run_migrations()
        .await
        .expect("failed to run libsql migrations");

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
