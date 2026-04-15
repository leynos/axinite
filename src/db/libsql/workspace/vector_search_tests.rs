//! Tests for libSQL workspace vector-search helpers.

use libsql::params;
use uuid::Uuid;

use super::super::LibSqlBackend;
use super::*;

#[test]
fn embedding_to_vector_json_serialises_embeddings_in_index_format() {
    assert_eq!(
        embedding_to_vector_json(&[1.0, -2.5, 0.25]),
        "[1,-2.5,0.25]"
    );
}

#[test]
fn rank_candidates_breaks_similarity_ties_by_chunk_id() {
    let earlier_chunk = Uuid::from_u128(1);
    let later_chunk = Uuid::from_u128(2);

    let results = rank_candidates(
        vec![
            Candidate {
                chunk_id: later_chunk,
                document_id: Uuid::new_v4(),
                document_path: "notes/later.md".to_string(),
                content: "later".to_string(),
                similarity: 0.9,
            },
            Candidate {
                chunk_id: earlier_chunk,
                document_id: Uuid::new_v4(),
                document_path: "notes/earlier.md".to_string(),
                content: "earlier".to_string(),
                similarity: 0.9,
            },
        ],
        2,
    );

    assert_eq!(results.len(), 2);
    assert_eq!(results[0].chunk_id, earlier_chunk);
    assert_eq!(results[0].rank, 1);
    assert_eq!(results[1].chunk_id, later_chunk);
    assert_eq!(results[1].rank, 2);
}

#[tokio::test]
async fn collect_vector_index_rows_skips_rows_with_invalid_uuids() {
    let backend = LibSqlBackend::new_memory()
        .await
        .expect("failed to create temp-file-backed backend");
    let conn = backend
        .connect()
        .await
        .expect("failed to open libsql connection");
    let valid_chunk_id = Uuid::new_v4();
    let valid_document_id = Uuid::new_v4();
    let rows = conn
        .query(
            "SELECT ?1, ?2, 'notes/good.md', 'good chunk' UNION ALL SELECT 'bad-uuid', ?3, 'notes/bad.md', 'bad chunk'",
            params![
                valid_chunk_id.to_string(),
                valid_document_id.to_string(),
                valid_document_id.to_string()
            ],
        )
        .await
        .expect("failed to query synthetic vector rows");

    let outcome = collect_vector_index_rows(rows, 5)
        .await
        .expect("vector row collection should succeed");

    let VectorSearchOutcome::Indexed(results) = outcome else {
        panic!("expected indexed vector-search outcome");
    };
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].chunk_id, valid_chunk_id);
    assert_eq!(results[0].document_id, valid_document_id);
    assert_eq!(results[0].rank, 1);
}
