//! Full-text-search helpers for libSQL workspace retrieval.

use libsql::params;

use super::super::get_text;
use crate::error::WorkspaceError;
use crate::workspace::RankedResult;

/// Execute full-text search and return ranked results.
///
/// Queries the memory_chunks_fts virtual table, joining with memory_chunks
/// and memory_documents to fetch chunk content and document paths. Assigns
/// rank based on result order.
pub(super) async fn fts_ranked_results(
    conn: &libsql::Connection,
    user_id: &str,
    agent_id: Option<&str>,
    query: &str,
    limit: i64,
) -> Result<Vec<RankedResult>, WorkspaceError> {
    let mut rows = conn
        .query(
            r#"
            SELECT c.id, c.document_id, d.path, c.content
            FROM memory_chunks_fts fts
            JOIN memory_chunks c ON c._rowid = fts.rowid
            JOIN memory_documents d ON d.id = c.document_id
            WHERE d.user_id = ?1 AND d.agent_id IS ?2
              AND memory_chunks_fts MATCH ?3
            ORDER BY rank
            LIMIT ?4
            "#,
            params![user_id, agent_id, query, limit],
        )
        .await
        .map_err(|e| WorkspaceError::SearchFailed {
            reason: format!("FTS query failed: {}", e),
        })?;

    let mut results = Vec::new();
    while let Some(row) = rows
        .next()
        .await
        .map_err(|e| WorkspaceError::SearchFailed {
            reason: format!("FTS row fetch failed: {}", e),
        })?
    {
        results.push(RankedResult {
            chunk_id: get_text(&row, 0).parse().unwrap_or_default(),
            document_id: get_text(&row, 1).parse().unwrap_or_default(),
            document_path: get_text(&row, 2),
            content: get_text(&row, 3),
            rank: results.len() as u32 + 1,
        });
    }
    Ok(results)
}
