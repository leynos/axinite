//! Workspace-related WorkspaceStore implementation for LibSqlBackend.

use std::collections::HashMap;

use libsql::params;
use uuid::Uuid;

use super::{
    LibSqlBackend, fmt_ts, get_i64, get_opt_text, get_opt_ts, get_text, get_ts,
    row_to_memory_document,
};
use crate::db::{HybridSearchParams, InsertChunkParams, NativeWorkspaceStore};
use crate::error::WorkspaceError;
use crate::workspace::{
    MemoryChunk, MemoryDocument, RankedResult, SearchResult, WorkspaceEntry, cosine_similarity,
    reciprocal_rank_fusion,
};

use chrono::Utc;

struct Candidate {
    chunk_id: Uuid,
    document_id: Uuid,
    document_path: String,
    content: String,
    similarity: f32,
}

enum VectorSearchOutcome {
    Indexed(Vec<RankedResult>),
    IndexUnavailable,
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
    // Sort by similarity descending
    candidates.sort_by(|a, b| {
        b.similarity
            .partial_cmp(&a.similarity)
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    let total_candidates = candidates.len();

    // Take top N and convert to RankedResult with 1-based rank
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
fn deserialize_embedding(blob: &[u8]) -> Vec<f32> {
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

            // Deserialize the embedding BLOB
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

            // Compute cosine similarity
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
    /// cosine similarity against the query embedding, and returns the top matches.
    /// This is used as a fallback when the vector index is not available (post-V9 migration).
    async fn brute_force_vector_search(
        &self,
        user_id: &str,
        agent_id: Option<Uuid>,
        embedding: &[f32],
        limit: usize,
    ) -> Result<Vec<RankedResult>, WorkspaceError> {
        let conn = self
            .connect()
            .await
            .map_err(|e| WorkspaceError::SearchFailed {
                reason: e.to_string(),
            })?;
        let agent_id_str = agent_id.map(|id| id.to_string());

        // Load all chunks with embeddings
        let mut rows = conn
            .query(
                r#"
                SELECT c.id, c.document_id, d.path, c.content, c.embedding
                FROM memory_chunks c
                JOIN memory_documents d ON d.id = c.document_id
                WHERE d.user_id = ?1 AND d.agent_id IS ?2
                  AND c.embedding IS NOT NULL
                "#,
                params![user_id, agent_id_str.as_deref()],
            )
            .await
            .map_err(|e| WorkspaceError::SearchFailed {
                reason: format!("Query failed: {}", e),
            })?;

        let candidates = self.collect_candidates(&mut rows, embedding).await?;
        Ok(rank_candidates(candidates, limit))
    }
}

/// Execute full-text search and return ranked results.
///
/// Queries the memory_chunks_fts virtual table, joining with memory_chunks
/// and memory_documents to fetch chunk content and document paths. Assigns
/// rank based on result order.
async fn fts_ranked_results(
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
        results.push(RankedResult {
            chunk_id: get_text(&row, 0).parse().unwrap_or_default(),
            document_id: get_text(&row, 1).parse().unwrap_or_default(),
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

/// Execute vector similarity search via libSQL's vector index.
///
/// Returns [`VectorSearchOutcome::IndexUnavailable`] when `vector_top_k(...)`
/// cannot run because the fixed-dimension vector index is missing, which is
/// the expected state after the V9 flexible-dimension migration.
async fn vector_ranked_results(
    conn: &libsql::Connection,
    user_id: &str,
    agent_id: Option<&str>,
    embedding: &[f32],
    limit: i64,
) -> Result<VectorSearchOutcome, WorkspaceError> {
    let vector_json = embedding_to_vector_json(embedding);

    // vector_top_k requires a libsql_vector_idx index. After the V9
    // migration the index is dropped (to support flexible embedding
    // dimensions), so this query may fail. The caller must then fall
    // back to brute-force cosine similarity.
    match conn
        .query(
            r#"
            SELECT c.id, c.document_id, d.path, c.content
            FROM vector_top_k('idx_memory_chunks_embedding', vector(?1), ?2) AS top_k
            JOIN memory_chunks c ON c._rowid = top_k.id
            JOIN memory_documents d ON d.id = c.document_id
            WHERE d.user_id = ?3 AND d.agent_id IS ?4
            "#,
            params![vector_json, limit, user_id, agent_id],
        )
        .await
    {
        Ok(rows) => collect_vector_index_rows(rows, limit).await,
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

impl NativeWorkspaceStore for LibSqlBackend {
    async fn get_document_by_path(
        &self,
        user_id: &str,
        agent_id: Option<Uuid>,
        path: &str,
    ) -> Result<MemoryDocument, WorkspaceError> {
        let conn = self
            .connect()
            .await
            .map_err(|e| WorkspaceError::SearchFailed {
                reason: e.to_string(),
            })?;
        let agent_id_str = agent_id.map(|id| id.to_string());
        let mut rows = conn
            .query(
                r#"
                SELECT id, user_id, agent_id, path, content,
                       created_at, updated_at, metadata
                FROM memory_documents
                WHERE user_id = ?1 AND agent_id IS ?2 AND path = ?3
                "#,
                params![user_id, agent_id_str.as_deref(), path],
            )
            .await
            .map_err(|e| WorkspaceError::SearchFailed {
                reason: format!("Query failed: {}", e),
            })?;

        match rows
            .next()
            .await
            .map_err(|e| WorkspaceError::SearchFailed {
                reason: format!("Query failed: {}", e),
            })? {
            Some(row) => Ok(row_to_memory_document(&row)),
            None => Err(WorkspaceError::DocumentNotFound {
                doc_type: path.to_string(),
                user_id: user_id.to_string(),
            }),
        }
    }

    async fn get_document_by_id(&self, id: Uuid) -> Result<MemoryDocument, WorkspaceError> {
        let conn = self
            .connect()
            .await
            .map_err(|e| WorkspaceError::SearchFailed {
                reason: e.to_string(),
            })?;
        let mut rows = conn
            .query(
                r#"
                SELECT id, user_id, agent_id, path, content,
                       created_at, updated_at, metadata
                FROM memory_documents WHERE id = ?1
                "#,
                params![id.to_string()],
            )
            .await
            .map_err(|e| WorkspaceError::SearchFailed {
                reason: format!("Query failed: {}", e),
            })?;

        match rows
            .next()
            .await
            .map_err(|e| WorkspaceError::SearchFailed {
                reason: format!("Query failed: {}", e),
            })? {
            Some(row) => Ok(row_to_memory_document(&row)),
            None => Err(WorkspaceError::DocumentNotFound {
                doc_type: "unknown".to_string(),
                user_id: "unknown".to_string(),
            }),
        }
    }

    async fn get_or_create_document_by_path(
        &self,
        user_id: &str,
        agent_id: Option<Uuid>,
        path: &str,
    ) -> Result<MemoryDocument, WorkspaceError> {
        // Try get
        match NativeWorkspaceStore::get_document_by_path(self, user_id, agent_id, path).await {
            Ok(doc) => return Ok(doc),
            Err(WorkspaceError::DocumentNotFound { .. }) => {}
            Err(e) => return Err(e),
        }

        // Create
        let conn = self
            .connect()
            .await
            .map_err(|e| WorkspaceError::SearchFailed {
                reason: e.to_string(),
            })?;
        let id = Uuid::new_v4();
        let agent_id_str = agent_id.map(|id| id.to_string());
        conn.execute(
            r#"
                INSERT INTO memory_documents (id, user_id, agent_id, path, content, metadata)
                VALUES (?1, ?2, ?3, ?4, '', '{}')
                ON CONFLICT DO NOTHING
                "#,
            params![id.to_string(), user_id, agent_id_str.as_deref(), path],
        )
        .await
        .map_err(|e| WorkspaceError::SearchFailed {
            reason: format!("Insert failed: {}", e),
        })?;

        NativeWorkspaceStore::get_document_by_path(self, user_id, agent_id, path).await
    }

    async fn update_document(&self, id: Uuid, content: &str) -> Result<(), WorkspaceError> {
        let conn = self
            .connect()
            .await
            .map_err(|e| WorkspaceError::SearchFailed {
                reason: e.to_string(),
            })?;
        let now = fmt_ts(&Utc::now());
        conn.execute(
            "UPDATE memory_documents SET content = ?2, updated_at = ?3 WHERE id = ?1",
            params![id.to_string(), content, now],
        )
        .await
        .map_err(|e| WorkspaceError::SearchFailed {
            reason: format!("Update failed: {}", e),
        })?;
        Ok(())
    }

    async fn delete_document_by_path(
        &self,
        user_id: &str,
        agent_id: Option<Uuid>,
        path: &str,
    ) -> Result<(), WorkspaceError> {
        let doc = NativeWorkspaceStore::get_document_by_path(self, user_id, agent_id, path).await?;
        NativeWorkspaceStore::delete_chunks(self, doc.id).await?;

        let conn = self
            .connect()
            .await
            .map_err(|e| WorkspaceError::SearchFailed {
                reason: e.to_string(),
            })?;
        let agent_id_str = agent_id.map(|id| id.to_string());
        conn.execute(
            "DELETE FROM memory_documents WHERE user_id = ?1 AND agent_id IS ?2 AND path = ?3",
            params![user_id, agent_id_str.as_deref(), path],
        )
        .await
        .map_err(|e| WorkspaceError::SearchFailed {
            reason: format!("Delete failed: {}", e),
        })?;
        Ok(())
    }

    async fn list_directory(
        &self,
        user_id: &str,
        agent_id: Option<Uuid>,
        directory: &str,
    ) -> Result<Vec<WorkspaceEntry>, WorkspaceError> {
        let conn = self
            .connect()
            .await
            .map_err(|e| WorkspaceError::SearchFailed {
                reason: e.to_string(),
            })?;
        let dir = if !directory.is_empty() && !directory.ends_with('/') {
            format!("{}/", directory)
        } else {
            directory.to_string()
        };

        let agent_id_str = agent_id.map(|id| id.to_string());
        let pattern = if dir.is_empty() {
            "%".to_string()
        } else {
            format!("{}%", dir)
        };

        let mut rows = conn
            .query(
                r#"
                SELECT path, updated_at, substr(content, 1, 200) as content_preview
                FROM memory_documents
                WHERE user_id = ?1 AND agent_id IS ?2
                  AND (?3 = '%' OR path LIKE ?3)
                ORDER BY path
                "#,
                params![user_id, agent_id_str.as_deref(), pattern],
            )
            .await
            .map_err(|e| WorkspaceError::SearchFailed {
                reason: format!("List directory failed: {}", e),
            })?;

        let mut entries_map: HashMap<String, WorkspaceEntry> = HashMap::new();

        while let Some(row) = rows
            .next()
            .await
            .map_err(|e| WorkspaceError::SearchFailed {
                reason: format!("Query failed: {}", e),
            })?
        {
            let full_path = get_text(&row, 0);
            let updated_at = get_opt_ts(&row, 1);
            let content_preview = get_opt_text(&row, 2);

            let relative = if dir.is_empty() {
                &full_path
            } else if let Some(stripped) = full_path.strip_prefix(&dir) {
                stripped
            } else {
                continue;
            };

            let child_name = if let Some(slash_pos) = relative.find('/') {
                &relative[..slash_pos]
            } else {
                relative
            };

            if child_name.is_empty() {
                continue;
            }

            let is_dir = relative.contains('/');
            let entry_path = if dir.is_empty() {
                child_name.to_string()
            } else {
                format!("{}{}", dir, child_name)
            };

            entries_map
                .entry(child_name.to_string())
                .and_modify(|e| {
                    if is_dir {
                        e.is_directory = true;
                        e.content_preview = None;
                    }
                    if let (Some(existing), Some(new)) = (&e.updated_at, &updated_at)
                        && new > existing
                    {
                        e.updated_at = Some(*new);
                    }
                })
                .or_insert(WorkspaceEntry {
                    path: entry_path,
                    is_directory: is_dir,
                    updated_at,
                    content_preview: if is_dir { None } else { content_preview },
                });
        }

        let mut entries: Vec<WorkspaceEntry> = entries_map.into_values().collect();
        entries.sort_by(|a, b| a.path.cmp(&b.path));
        Ok(entries)
    }

    async fn list_all_paths(
        &self,
        user_id: &str,
        agent_id: Option<Uuid>,
    ) -> Result<Vec<String>, WorkspaceError> {
        let conn = self
            .connect()
            .await
            .map_err(|e| WorkspaceError::SearchFailed {
                reason: e.to_string(),
            })?;
        let agent_id_str = agent_id.map(|id| id.to_string());
        let mut rows = conn
            .query(
                "SELECT path FROM memory_documents WHERE user_id = ?1 AND agent_id IS ?2 ORDER BY path",
                params![user_id, agent_id_str.as_deref()],
            )
            .await
            .map_err(|e| WorkspaceError::SearchFailed {
                reason: format!("List paths failed: {}", e),
            })?;

        let mut paths = Vec::new();
        while let Some(row) = rows
            .next()
            .await
            .map_err(|e| WorkspaceError::SearchFailed {
                reason: format!("Query failed: {}", e),
            })?
        {
            paths.push(get_text(&row, 0));
        }
        Ok(paths)
    }

    async fn list_documents(
        &self,
        user_id: &str,
        agent_id: Option<Uuid>,
    ) -> Result<Vec<MemoryDocument>, WorkspaceError> {
        let conn = self
            .connect()
            .await
            .map_err(|e| WorkspaceError::SearchFailed {
                reason: e.to_string(),
            })?;
        let agent_id_str = agent_id.map(|id| id.to_string());
        let mut rows = conn
            .query(
                r#"
                SELECT id, user_id, agent_id, path, content,
                       created_at, updated_at, metadata
                FROM memory_documents
                WHERE user_id = ?1 AND agent_id IS ?2
                ORDER BY updated_at DESC
                "#,
                params![user_id, agent_id_str.as_deref()],
            )
            .await
            .map_err(|e| WorkspaceError::SearchFailed {
                reason: format!("Query failed: {}", e),
            })?;

        let mut docs = Vec::new();
        while let Some(row) = rows
            .next()
            .await
            .map_err(|e| WorkspaceError::SearchFailed {
                reason: format!("Query failed: {}", e),
            })?
        {
            docs.push(row_to_memory_document(&row));
        }
        Ok(docs)
    }

    async fn delete_chunks(&self, document_id: Uuid) -> Result<(), WorkspaceError> {
        let conn = self
            .connect()
            .await
            .map_err(|e| WorkspaceError::ChunkingFailed {
                reason: e.to_string(),
            })?;
        conn.execute(
            "DELETE FROM memory_chunks WHERE document_id = ?1",
            params![document_id.to_string()],
        )
        .await
        .map_err(|e| WorkspaceError::ChunkingFailed {
            reason: format!("Delete failed: {}", e),
        })?;
        Ok(())
    }

    async fn insert_chunk(&self, params: InsertChunkParams<'_>) -> Result<Uuid, WorkspaceError> {
        let InsertChunkParams {
            document_id,
            chunk_index,
            content,
            embedding,
        } = params;
        let conn = self
            .connect()
            .await
            .map_err(|e| WorkspaceError::ChunkingFailed {
                reason: e.to_string(),
            })?;
        let id = Uuid::new_v4();
        let chunk_index = i64::from(chunk_index);
        let embedding_blob = embedding.map(|e| {
            let bytes: Vec<u8> = e.iter().flat_map(|f| f.to_le_bytes()).collect();
            bytes
        });

        conn.execute(
            r#"
                INSERT INTO memory_chunks (id, document_id, chunk_index, content, embedding)
                VALUES (?1, ?2, ?3, ?4, ?5)
                "#,
            params![
                id.to_string(),
                document_id.to_string(),
                chunk_index,
                content,
                embedding_blob.map(libsql::Value::Blob),
            ],
        )
        .await
        .map_err(|e| WorkspaceError::ChunkingFailed {
            reason: format!("Insert failed: {}", e),
        })?;
        Ok(id)
    }

    async fn update_chunk_embedding(
        &self,
        chunk_id: Uuid,
        embedding: &[f32],
    ) -> Result<(), WorkspaceError> {
        let conn = self
            .connect()
            .await
            .map_err(|e| WorkspaceError::EmbeddingFailed {
                reason: e.to_string(),
            })?;
        let bytes: Vec<u8> = embedding.iter().flat_map(|f| f.to_le_bytes()).collect();

        conn.execute(
            "UPDATE memory_chunks SET embedding = ?2 WHERE id = ?1",
            params![chunk_id.to_string(), libsql::Value::Blob(bytes)],
        )
        .await
        .map_err(|e| WorkspaceError::EmbeddingFailed {
            reason: format!("Update failed: {}", e),
        })?;
        Ok(())
    }

    async fn get_chunks_without_embeddings(
        &self,
        user_id: &str,
        agent_id: Option<Uuid>,
        limit: usize,
    ) -> Result<Vec<MemoryChunk>, WorkspaceError> {
        let conn = self
            .connect()
            .await
            .map_err(|e| WorkspaceError::SearchFailed {
                reason: e.to_string(),
            })?;
        let agent_id_str = agent_id.map(|id| id.to_string());
        let mut rows = conn
            .query(
                r#"
                SELECT c.id, c.document_id, c.chunk_index, c.content, c.created_at
                FROM memory_chunks c
                JOIN memory_documents d ON d.id = c.document_id
                WHERE d.user_id = ?1 AND d.agent_id IS ?2
                  AND c.embedding IS NULL
                LIMIT ?3
                "#,
                params![user_id, agent_id_str.as_deref(), limit as i64],
            )
            .await
            .map_err(|e| WorkspaceError::SearchFailed {
                reason: format!("Query failed: {}", e),
            })?;

        let mut chunks = Vec::new();
        while let Some(row) = rows
            .next()
            .await
            .map_err(|e| WorkspaceError::SearchFailed {
                reason: format!("Query failed: {}", e),
            })?
        {
            let chunk_index =
                u32::try_from(get_i64(&row, 2)).map_err(|_| WorkspaceError::SearchFailed {
                    reason: "memory_chunks.chunk_index must be non-negative".to_string(),
                })?;
            chunks.push(MemoryChunk {
                id: get_text(&row, 0).parse().unwrap_or_default(),
                document_id: get_text(&row, 1).parse().unwrap_or_default(),
                chunk_index,
                content: get_text(&row, 3),
                embedding: None,
                created_at: get_ts(&row, 4),
            });
        }
        Ok(chunks)
    }

    async fn hybrid_search(
        &self,
        params: HybridSearchParams<'_>,
    ) -> Result<Vec<SearchResult>, WorkspaceError> {
        let HybridSearchParams {
            user_id,
            agent_id,
            query,
            embedding,
            config,
        } = params;
        let conn = self
            .connect()
            .await
            .map_err(|e| WorkspaceError::SearchFailed {
                reason: e.to_string(),
            })?;
        let agent_id_str = agent_id.map(|id| id.to_string());
        let pre_limit = config.pre_fusion_limit as i64;

        let fts_results = if config.use_fts {
            let results =
                fts_ranked_results(&conn, user_id, agent_id_str.as_deref(), query, pre_limit)
                    .await?;
            tracing::debug!(
                "FTS search returned {} results (pre-fusion limit: {})",
                results.len(),
                pre_limit
            );
            results
        } else {
            Vec::new()
        };

        let vector_results = if config.use_vector {
            if let Some(emb) = embedding {
                match vector_ranked_results(&conn, user_id, agent_id_str.as_deref(), emb, pre_limit)
                    .await?
                {
                    VectorSearchOutcome::Indexed(results) => results,
                    VectorSearchOutcome::IndexUnavailable => {
                        tracing::info!("Using brute-force vector search (no vector index)");
                        self.brute_force_vector_search(user_id, agent_id, emb, pre_limit as usize)
                            .await
                            .map_err(|e| {
                                tracing::warn!("Brute-force vector search failed: {e}");
                                e
                            })?
                    }
                }
            } else {
                Vec::new()
            }
        } else {
            if embedding.is_some() {
                tracing::warn!(
                    "Embedding provided but vector search is disabled in config; using FTS-only results"
                );
            }
            Vec::new()
        };

        Ok(reciprocal_rank_fusion(fts_results, vector_results, config))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
        // 7 bytes is not a multiple of 4
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
        use crate::db::{InsertChunkParams, NativeDatabase, NativeWorkspaceStore};
        use crate::workspace::SearchConfig;

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
        let vector_outcome = vector_ranked_results(&conn, "default", None, &[1.0, 0.0, 0.0], 5)
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
        use crate::db::{InsertChunkParams, NativeDatabase, NativeWorkspaceStore};

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
            .brute_force_vector_search("default", None, &[1.0, 0.0, 0.0], 10)
            .await
            .expect("failed to run brute-force vector search");

        assert_eq!(results.len(), 1);
        assert_eq!(results[0].content, "same-dimension chunk");
    }
}
