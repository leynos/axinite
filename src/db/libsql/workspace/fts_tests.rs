//! Tests for libSQL workspace full-text search helpers.

use super::super::LibSqlBackend;
use super::*;
use crate::db::{InsertChunkParams, NativeDatabase, NativeWorkspaceStore};

#[tokio::test]
async fn fts_ranked_results_returns_ranked_matches() {
    let backend = LibSqlBackend::new_memory()
        .await
        .expect("failed to create temp-file-backed backend");
    backend
        .run_migrations()
        .await
        .expect("failed to run libsql migrations");
    let document = backend
        .get_or_create_document_by_path("default", None, "notes/fts.md")
        .await
        .expect("failed to create FTS document");
    backend
        .insert_chunk(InsertChunkParams {
            document_id: document.id,
            chunk_index: 0,
            content: "semantic retrieval through full text",
            embedding: None,
        })
        .await
        .expect("failed to insert FTS chunk");
    let conn = backend
        .connect()
        .await
        .expect("failed to open libsql connection");

    let results = fts_ranked_results(
        &conn,
        FtsSearchParams {
            user_id: "default",
            agent_id: None,
            query: "semantic",
            limit: 5,
        },
    )
    .await
    .expect("FTS query should succeed");

    assert_eq!(results.len(), 1);
    assert_eq!(results[0].document_path, "notes/fts.md");
    assert_eq!(results[0].content, "semantic retrieval through full text");
    assert_eq!(results[0].rank, 1);
}

#[tokio::test]
async fn fts_ranked_results_surfaces_query_errors() {
    let backend = LibSqlBackend::new_memory()
        .await
        .expect("failed to create temp-file-backed backend");
    let conn = backend
        .connect()
        .await
        .expect("failed to open libsql connection");

    let error = fts_ranked_results(
        &conn,
        FtsSearchParams {
            user_id: "default",
            agent_id: None,
            query: "semantic",
            limit: 5,
        },
    )
    .await
    .expect_err("FTS query should fail before migrations");

    assert!(matches!(
        error,
        crate::error::WorkspaceError::SearchFailed { reason }
            if reason.starts_with("FTS query failed:")
    ));
}
