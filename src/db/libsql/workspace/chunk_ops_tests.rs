//! Tests for libSQL workspace chunk helpers.

use chrono::{TimeZone, Utc};
use libsql::params;
use uuid::Uuid;

use super::*;
use crate::db::NativeDatabase;

#[tokio::test]
async fn parse_chunk_row_maps_valid_rows() {
    let chunk_id = Uuid::new_v4();
    let document_id = Uuid::new_v4();
    let backend = LibSqlBackend::new_memory()
        .await
        .expect("failed to create temp-file-backed backend");
    let conn = backend
        .connect()
        .await
        .expect("failed to open libsql connection");
    let mut rows = conn
        .query(
            "SELECT ?1, ?2, ?3, ?4, ?5",
            params![
                chunk_id.to_string(),
                document_id.to_string(),
                7i64,
                "chunk body",
                "2026-03-07T12:34:56.000Z"
            ],
        )
        .await
        .expect("failed to query literal chunk row");
    let row = rows
        .next()
        .await
        .expect("failed to fetch literal chunk row")
        .expect("expected one literal chunk row");

    let chunk = parse_chunk_row(row)
        .expect("valid chunk row should parse")
        .expect("valid chunk row should not be skipped");

    assert_eq!(chunk.id, chunk_id);
    assert_eq!(chunk.document_id, document_id);
    assert_eq!(chunk.chunk_index, 7);
    assert_eq!(chunk.content, "chunk body");
    assert_eq!(
        chunk.created_at,
        Utc.with_ymd_and_hms(2026, 3, 7, 12, 34, 56)
            .single()
            .expect("timestamp should be valid"),
    );
}

#[tokio::test]
async fn parse_chunk_row_skips_invalid_chunk_uuid() {
    let backend = LibSqlBackend::new_memory()
        .await
        .expect("failed to create temp-file-backed backend");
    let conn = backend
        .connect()
        .await
        .expect("failed to open libsql connection");
    let mut rows = conn
        .query(
            "SELECT 'not-a-uuid', ?1, 0, 'chunk body', '2026-03-07T12:34:56.000Z'",
            params![Uuid::new_v4().to_string()],
        )
        .await
        .expect("failed to query literal chunk row");
    let row = rows
        .next()
        .await
        .expect("failed to fetch literal chunk row")
        .expect("expected one literal chunk row");

    assert!(
        parse_chunk_row(row)
            .expect("invalid chunk id should be skipped cleanly")
            .is_none()
    );
}

#[tokio::test]
async fn parse_chunk_row_rejects_negative_chunk_index() {
    let backend = LibSqlBackend::new_memory()
        .await
        .expect("failed to create temp-file-backed backend");
    let conn = backend
        .connect()
        .await
        .expect("failed to open libsql connection");
    let mut rows = conn
        .query(
            "SELECT ?1, ?2, -1, 'chunk body', '2026-03-07T12:34:56.000Z'",
            params![Uuid::new_v4().to_string(), Uuid::new_v4().to_string()],
        )
        .await
        .expect("failed to query literal chunk row");
    let row = rows
        .next()
        .await
        .expect("failed to fetch literal chunk row")
        .expect("expected one literal chunk row");

    let error = parse_chunk_row(row).expect_err("negative chunk index must fail");
    assert!(matches!(
        error,
        WorkspaceError::SearchFailed { reason }
            if reason == "memory_chunks.chunk_index must be non-negative"
    ));
}

#[tokio::test]
async fn insert_chunk_stores_empty_embeddings_as_null() {
    let backend = LibSqlBackend::new_memory()
        .await
        .expect("failed to create temp-file-backed backend");
    backend
        .run_migrations()
        .await
        .expect("failed to run libsql migrations");
    let document_id = Uuid::new_v4();
    let conn = backend
        .connect()
        .await
        .expect("failed to open libsql connection");
    conn.execute(
        "INSERT INTO memory_documents (id, user_id, agent_id, path, content, metadata) VALUES (?1, 'default', NULL, 'notes/chunk.md', '', '{}')",
        params![document_id.to_string()],
    )
    .await
    .expect("failed to insert document");

    let chunk_id = insert_chunk(
        &backend,
        crate::db::InsertChunkParams {
            document_id,
            chunk_index: 0,
            content: "chunk body",
            embedding: Some(&[]),
        },
    )
    .await
    .expect("failed to insert chunk with empty embedding");

    let mut rows = conn
        .query(
            "SELECT embedding FROM memory_chunks WHERE id = ?1",
            params![chunk_id.to_string()],
        )
        .await
        .expect("failed to query inserted chunk");
    let row = rows
        .next()
        .await
        .expect("failed to fetch inserted chunk row")
        .expect("expected inserted chunk row");
    assert!(matches!(
        row.get_value(0).expect("failed to read embedding value"),
        libsql::Value::Null
    ));
}
