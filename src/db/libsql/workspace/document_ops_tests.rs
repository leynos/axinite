//! Tests for libSQL workspace document helpers.

use libsql::params;
use uuid::Uuid;

use super::*;
use crate::db::{NativeDatabase, NativeWorkspaceStore};

#[tokio::test]
async fn document_from_row_or_not_found_maps_present_rows() {
    let id = Uuid::new_v4();
    let backend = LibSqlBackend::new_memory()
        .await
        .expect("failed to create temp-file-backed backend");
    let conn = backend
        .connect()
        .await
        .expect("failed to open libsql connection");
    let mut rows = conn
        .query(
            "SELECT ?1, 'default', NULL, 'notes/doc.md', 'hello', '2026-03-07T12:34:56.000Z', '2026-03-07T12:35:56.000Z', '{\"kind\":\"note\"}'",
            params![id.to_string()],
        )
        .await
        .expect("failed to query literal document row");
    let row = rows
        .next()
        .await
        .expect("failed to fetch literal document row")
        .expect("expected one literal document row");

    let document = document_from_row_or_not_found(Some(row), "notes/doc.md", "default")
        .expect("present row should map to document");

    assert_eq!(document.id, id);
    assert_eq!(document.user_id, "default");
    assert_eq!(document.path, "notes/doc.md");
    assert_eq!(document.content, "hello");
}

#[test]
fn document_from_row_or_not_found_returns_not_found_error() {
    let error = document_from_row_or_not_found(None, "notes/missing.md", "default")
        .expect_err("missing row should become not-found");
    assert!(matches!(
        error,
        WorkspaceError::DocumentNotFound { doc_type, user_id }
            if doc_type == "notes/missing.md" && user_id == "default"
    ));
}

#[tokio::test]
async fn get_document_by_path_returns_not_found_for_missing_document() {
    let backend = LibSqlBackend::new_memory()
        .await
        .expect("failed to create temp-file-backed backend");
    backend
        .run_migrations()
        .await
        .expect("failed to run libsql migrations");

    let error = get_document_by_path(
        &backend,
        &AgentScope {
            user_id: "default",
            agent_id: None,
        },
        "notes/missing.md",
    )
    .await
    .expect_err("missing document lookup should fail");
    assert!(matches!(
        error,
        WorkspaceError::DocumentNotFound { doc_type, user_id }
            if doc_type == "notes/missing.md" && user_id == "default"
    ));
}

#[tokio::test]
async fn list_directory_merges_file_and_directory_entries() {
    let backend = LibSqlBackend::new_memory()
        .await
        .expect("failed to create temp-file-backed backend");
    backend
        .run_migrations()
        .await
        .expect("failed to run libsql migrations");
    backend
        .get_or_create_document_by_path("default", None, "notes/alpha.md")
        .await
        .expect("failed to create alpha doc");
    backend
        .get_or_create_document_by_path("default", None, "notes/nested/beta.md")
        .await
        .expect("failed to create beta doc");

    let entries = list_directory(
        &backend,
        &AgentScope {
            user_id: "default",
            agent_id: None,
        },
        "notes",
    )
    .await
    .expect("failed to list directory");

    assert_eq!(entries.len(), 2);
    assert_eq!(entries[0].path, "notes/alpha.md");
    assert!(!entries[0].is_directory);
    assert_eq!(entries[1].path, "notes/nested");
    assert!(entries[1].is_directory);
}
