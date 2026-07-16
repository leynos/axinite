//! Tests for embedding serialization and vector/hybrid search behaviour.

use super::super::vector_search::{
    VectorIndexQuery, VectorSearchOutcome, VectorSearchQuery, deserialize_embedding,
    vector_ranked_results,
};
use super::helpers::{assert_embedding_approx_eq, assert_sole_search_result, setup_backend};
use crate::db::{HybridSearchParams, InsertChunkParams, NativeWorkspaceStore};
use crate::workspace::SearchConfig;

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

#[test]
fn test_embedding_to_vector_json_formats_floats_as_json_array() {
    use super::super::vector_search::embedding_to_vector_json;

    let result = embedding_to_vector_json(&[1.0, -2.5, 0.0]);

    assert!(
        result.starts_with('['),
        "JSON array must start with '[', got: {result}"
    );
    assert!(
        result.ends_with(']'),
        "JSON array must end with ']', got: {result}"
    );
    // The negative float must be preserved faithfully.
    assert!(
        result.contains("-2.5") || result.contains("-2."),
        "must serialize the negative float, got: {result}"
    );

    // An empty slice must produce "[]".
    let empty = embedding_to_vector_json(&[]);
    assert_eq!(empty, "[]", "empty embedding must serialize as '[]'");
}

// This test also validates the `collect_vector_index_rows` →
// IndexUnavailable path: the pre-condition assertion confirms
// vector_ranked_results returns IndexUnavailable before the brute-force
// fallback assertions begin.
#[tokio::test]
async fn hybrid_search_uses_brute_force_when_vector_index_is_unavailable() {
    let backend = setup_backend().await.expect("failed to set up backend");

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
    let backend = setup_backend().await.expect("failed to set up backend");

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
    let backend = setup_backend().await.expect("failed to set up backend");

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
