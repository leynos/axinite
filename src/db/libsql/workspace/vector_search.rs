//! Vector-search helpers for libSQL workspace retrieval.

use libsql::params;
use uuid::Uuid;

use super::super::{LibSqlBackend, get_text};
use crate::error::WorkspaceError;
use crate::workspace::{RankedResult, cosine_similarity};

struct Candidate {
    chunk_id: Uuid,
    document_id: Uuid,
    document_path: String,
    content: String,
    similarity: f32,
}

pub(super) enum VectorSearchOutcome {
    Indexed(Vec<RankedResult>),
    IndexUnavailable,
}

/// Scoped query parameters shared by vector-search helpers.
///
/// Bundles the user/agent scope with the query embedding so callers
/// pass a single cohesive object rather than three separate arguments.
pub(super) struct VectorSearchQuery<'a> {
    pub(super) user_id: &'a str,
    pub(super) agent_id: Option<Uuid>,
    pub(super) embedding: &'a [f32],
}

fn is_missing_vector_index_error(error: &libsql::Error) -> bool {
    let sqlite_message = match error {
        libsql::Error::SqliteFailure(_, message)
        | libsql::Error::RemoteSqliteFailure(_, _, message) => message,
        _ => return false,
    };

    let error_message = sqlite_message.to_ascii_lowercase();

    error_message.contains("vector_top_k")
        || error_message.contains("no such function")
        || error_message.contains("idx_memory_chunks_embedding")
        || error_message.contains("failed to parse vector index parameters")
}

fn rank_candidates(mut candidates: Vec<Candidate>, limit: usize) -> Vec<RankedResult> {
    candidates.sort_by(|a, b| {
        b.similarity
            .partial_cmp(&a.similarity)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.chunk_id.cmp(&b.chunk_id))
    });

    let total_candidates = candidates.len();
    let results: Vec<_> = candidates
        .into_iter()
        .take(limit)
        .enumerate()
        .map(|(idx, c)| RankedResult {
            chunk_id: c.chunk_id,
            document_id: c.document_id,
            document_path: c.document_path,
            content: c.content,
            rank: (idx + 1) as u32,
        })
        .collect();

    tracing::debug!(
        "Brute-force vector search scanned {} candidates, returned {} results",
        total_candidates,
        results.len()
    );

    results
}

/// Deserialize an embedding from a BLOB (4-byte little-endian f32 values).
///
/// Returns an empty vector if the blob length is not a multiple of 4.
pub(super) fn deserialize_embedding(blob: &[u8]) -> Vec<f32> {
    if !blob.len().is_multiple_of(4) {
        tracing::warn!(
            "Embedding blob length {} is not a multiple of 4; skipping",
            blob.len()
        );
        return Vec::new();
    }

    blob.chunks_exact(4)
        .map(|chunk| {
            let bytes = [chunk[0], chunk[1], chunk[2], chunk[3]];
            f32::from_le_bytes(bytes)
        })
        .collect()
}

impl LibSqlBackend {
    async fn collect_candidates(
        &self,
        rows: &mut libsql::Rows,
        query_embedding: &[f32],
    ) -> Result<Vec<Candidate>, WorkspaceError> {
        let mut candidates = Vec::new();
        let mut skipped_mismatched_dims = 0usize;
        while let Some(row) = rows
            .next()
            .await
            .map_err(|e| WorkspaceError::SearchFailed {
                reason: format!("Row fetch failed: {}", e),
            })?
        {
            let chunk_id: Uuid = match get_text(&row, 0).parse() {
                Ok(id) => id,
                Err(e) => {
                    tracing::warn!("Invalid chunk_id UUID in memory_chunks: {e}");
                    continue;
                }
            };
            let document_id: Uuid = match get_text(&row, 1).parse() {
                Ok(id) => id,
                Err(e) => {
                    tracing::warn!("Invalid document_id UUID in memory_chunks: {e}");
                    continue;
                }
            };
            let document_path = get_text(&row, 2);
            let content = get_text(&row, 3);
            let embedding_blob = match row.get_value(4) {
                Ok(libsql::Value::Blob(bytes)) => bytes,
                _ => continue,
            };
            let chunk_embedding = deserialize_embedding(&embedding_blob);
            if chunk_embedding.is_empty() {
                continue;
            }
            if chunk_embedding.len() != query_embedding.len() {
                skipped_mismatched_dims += 1;
                continue;
            }

            let similarity = cosine_similarity(query_embedding, &chunk_embedding);
            candidates.push(Candidate {
                chunk_id,
                document_id,
                document_path,
                content,
                similarity,
            });
        }

        if skipped_mismatched_dims > 0 {
            tracing::debug!(
                "Brute-force vector search skipped {} candidates with embedding dimension mismatches (query dimension: {})",
                skipped_mismatched_dims,
                query_embedding.len()
            );
        }

        Ok(candidates)
    }

    /// Brute-force vector search using cosine similarity in Rust.
    ///
    /// Loads all chunks with embeddings for the given user/agent, computes
    /// cosine similarity against the query embedding, and returns the top
    /// matches. This is used as a fallback when the vector index is not
    /// available (post-V9 migration).
    pub(super) async fn brute_force_vector_search(
        &self,
        query: VectorSearchQuery<'_>,
        limit: usize,
    ) -> Result<Vec<RankedResult>, WorkspaceError> {
        let conn = self
            .connect()
            .await
            .map_err(|e| WorkspaceError::SearchFailed {
                reason: e.to_string(),
            })?;
        let agent_id_str = query.agent_id.map(|id| id.to_string());
        let mut rows = conn
            .query(
                r#"
                SELECT c.id, c.document_id, d.path, c.content, c.embedding
                FROM memory_chunks c
                JOIN memory_documents d ON d.id = c.document_id
                WHERE d.user_id = ?1 AND d.agent_id IS ?2
                  AND c.embedding IS NOT NULL
                "#,
                params![query.user_id, agent_id_str.as_deref()],
            )
            .await
            .map_err(|e| WorkspaceError::SearchFailed {
                reason: format!("Query failed: {}", e),
            })?;

        let candidates = self.collect_candidates(&mut rows, query.embedding).await?;
        Ok(rank_candidates(candidates, limit))
    }
}

fn embedding_to_vector_json(embedding: &[f32]) -> String {
    format!(
        "[{}]",
        embedding
            .iter()
            .map(|f| f.to_string())
            .collect::<Vec<_>>()
            .join(",")
    )
}

async fn collect_vector_index_rows(
    mut rows: libsql::Rows,
    limit: i64,
) -> Result<VectorSearchOutcome, WorkspaceError> {
    let mut results = Vec::new();
    while let Some(row) = match rows.next().await {
        Ok(row) => row,
        Err(e) => {
            if is_missing_vector_index_error(&e) {
                tracing::debug!(
                    "Vector index row fetch failed, brute-force fallback required: {e}"
                );
                return Ok(VectorSearchOutcome::IndexUnavailable);
            }

            return Err(WorkspaceError::SearchFailed {
                reason: format!("Vector index row fetch failed: {e}"),
            });
        }
    } {
        let chunk_id: Uuid = match get_text(&row, 0).parse() {
            Ok(id) => id,
            Err(e) => {
                tracing::warn!("Invalid chunk_id UUID in memory_chunks: {e}");
                continue;
            }
        };
        let document_id: Uuid = match get_text(&row, 1).parse() {
            Ok(id) => id,
            Err(e) => {
                tracing::warn!("Invalid document_id UUID in memory_documents: {e}");
                continue;
            }
        };
        results.push(RankedResult {
            chunk_id,
            document_id,
            document_path: get_text(&row, 2),
            content: get_text(&row, 3),
            rank: results.len() as u32 + 1,
        });
    }
    tracing::debug!(
        "libSQL vector index search returned {} results (pre-fusion limit: {})",
        results.len(),
        limit
    );
    Ok(VectorSearchOutcome::Indexed(results))
}

/// Parameters for a vector-index similarity query.
///
/// Groups the search-intent arguments for [`vector_ranked_results`] to keep
/// its arity within the project limit of four.
pub(super) struct VectorIndexQuery<'a> {
    pub(super) user_id: &'a str,
    pub(super) agent_id: Option<&'a str>,
    pub(super) embedding: &'a [f32],
    pub(super) limit: i64,
}

/// Execute vector similarity search via libSQL's vector index.
///
/// Returns [`VectorSearchOutcome::IndexUnavailable`] when `vector_top_k(...)`
/// cannot run because the fixed-dimension vector index is missing, which is
/// the expected state after the V9 flexible-dimension migration.
pub(super) async fn vector_ranked_results(
    conn: &libsql::Connection,
    query: &VectorIndexQuery<'_>,
) -> Result<VectorSearchOutcome, WorkspaceError> {
    let vector_json = embedding_to_vector_json(query.embedding);

    match conn
        .query(
            r#"
            SELECT c.id, c.document_id, d.path, c.content
            FROM vector_top_k('idx_memory_chunks_embedding', vector(?1), ?2) AS top_k
            JOIN memory_chunks c ON c._rowid = top_k.id
            JOIN memory_documents d ON d.id = c.document_id
            WHERE d.user_id = ?3 AND d.agent_id IS ?4
            "#,
            params![vector_json, query.limit, query.user_id, query.agent_id],
        )
        .await
    {
        Ok(rows) => collect_vector_index_rows(rows, query.limit).await,
        Err(e) => {
            if is_missing_vector_index_error(&e) {
                tracing::debug!(
                    "Vector index query failed (expected after V9 migration), \
                     brute-force fallback required: {e}"
                );
                Ok(VectorSearchOutcome::IndexUnavailable)
            } else {
                Err(WorkspaceError::SearchFailed {
                    reason: format!("Vector index query failed: {e}"),
                })
            }
        }
    }
}
