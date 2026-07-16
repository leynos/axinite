//! Shared fixtures and assertion helpers for the workspace-store tests.

use anyhow::Context as _;

use crate::db::libsql::LibSqlBackend;
use crate::db::{NativeDatabase, NativeWorkspaceStore};

/// Assert that `actual` has the same length as `expected` and that every
/// element is within floating-point tolerance.
pub(super) fn assert_embedding_approx_eq(actual: &[f32], expected: &[f32]) {
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
pub(super) fn assert_sole_search_result(
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
pub(super) async fn setup_backend() -> anyhow::Result<LibSqlBackend> {
    let backend = LibSqlBackend::new_memory()
        .await
        .context("failed to create in-memory libsql backend")?;
    backend
        .run_migrations()
        .await
        .context("failed to run libsql migrations")?;
    Ok(backend)
}

/// Create a document at `path` for the default user scope, ready for use in
/// unit tests.
pub(super) async fn create_test_document(
    backend: &LibSqlBackend,
    path: &str,
) -> anyhow::Result<crate::workspace::MemoryDocument> {
    backend
        .get_or_create_document_by_path("default", None, path)
        .await
        .with_context(|| format!("failed to create test document at '{path}'"))
}

/// Assert that `result` is the `DocumentNotFound` error variant.
pub(super) fn assert_document_not_found<T: std::fmt::Debug>(
    result: Result<T, crate::error::WorkspaceError>,
) {
    assert!(
        matches!(
            result,
            Err(crate::error::WorkspaceError::DocumentNotFound { .. })
        ),
        "expected DocumentNotFound, got {:?}",
        result
    );
}
