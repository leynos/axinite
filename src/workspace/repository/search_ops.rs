//! Hybrid, full-text, and vector search operations for the workspace repository.

use pgvector::Vector;
use uuid::Uuid;

use crate::error::WorkspaceError;
use crate::workspace::search::{RankedResult, SearchConfig, SearchResult, reciprocal_rank_fusion};

use super::Repository;

impl Repository {
    /// Perform hybrid search combining FTS and vector similarity.
    pub async fn hybrid_search(
        &self,
        user_id: &str,
        agent_id: Option<Uuid>,
        query: &str,
        embedding: Option<&[f32]>,
        config: &SearchConfig,
    ) -> Result<Vec<SearchResult>, WorkspaceError> {
        let fts_results = if config.use_fts {
            let results = self
                .fts_search(user_id, agent_id, query, config.pre_fusion_limit)
                .await?;
            tracing::debug!(
                "FTS search returned {} results (pre-fusion limit: {})",
                results.len(),
                config.pre_fusion_limit
            );
            results
        } else {
            Vec::new()
        };

        let vector_results = if config.use_vector {
            if let Some(embedding) = embedding {
                let results = self
                    .vector_search(user_id, agent_id, embedding, config.pre_fusion_limit)
                    .await?;
                tracing::debug!(
                    "pgvector search returned {} results (pre-fusion limit: {})",
                    results.len(),
                    config.pre_fusion_limit
                );
                results
            } else {
                Vec::new()
            }
        } else {
            Vec::new()
        };

        Ok(reciprocal_rank_fusion(fts_results, vector_results, config))
    }

    /// Full-text search using PostgreSQL ts_rank_cd.
    async fn fts_search(
        &self,
        user_id: &str,
        agent_id: Option<Uuid>,
        query: &str,
        limit: usize,
    ) -> Result<Vec<RankedResult>, WorkspaceError> {
        let conn = self.conn().await?;

        let rows = conn
            .query(
                r#"
                SELECT c.id as chunk_id, c.document_id, d.path as document_path, c.content,
                       ts_rank_cd(c.content_tsv, plainto_tsquery('english', $3)) as rank
                FROM memory_chunks c
                JOIN memory_documents d ON d.id = c.document_id
                WHERE d.user_id = $1 AND d.agent_id IS NOT DISTINCT FROM $2
                  AND c.content_tsv @@ plainto_tsquery('english', $3)
                ORDER BY rank DESC
                LIMIT $4
                "#,
                &[&user_id, &agent_id, &query, &(limit as i64)],
            )
            .await
            .map_err(|e| WorkspaceError::SearchFailed {
                reason: format!("FTS query failed: {}", e),
            })?;

        Ok(rows
            .iter()
            .enumerate()
            .map(|(i, row)| RankedResult {
                chunk_id: row.get("chunk_id"),
                document_id: row.get("document_id"),
                document_path: row.get("document_path"),
                content: row.get("content"),
                rank: (i + 1) as u32,
            })
            .collect())
    }

    /// Vector similarity search using pgvector cosine distance.
    async fn vector_search(
        &self,
        user_id: &str,
        agent_id: Option<Uuid>,
        embedding: &[f32],
        limit: usize,
    ) -> Result<Vec<RankedResult>, WorkspaceError> {
        let conn = self.conn().await?;
        let embedding_vec = Vector::from(embedding.to_vec());

        let rows = conn
            .query(
                r#"
                SELECT c.id as chunk_id, c.document_id, d.path as document_path, c.content,
                       1 - (c.embedding <=> $3) as similarity
                FROM memory_chunks c
                JOIN memory_documents d ON d.id = c.document_id
                WHERE d.user_id = $1 AND d.agent_id IS NOT DISTINCT FROM $2
                  AND c.embedding IS NOT NULL
                ORDER BY c.embedding <=> $3
                LIMIT $4
                "#,
                &[&user_id, &agent_id, &embedding_vec, &(limit as i64)],
            )
            .await
            .map_err(|e| WorkspaceError::SearchFailed {
                reason: format!("Vector query failed: {}", e),
            })?;

        Ok(rows
            .iter()
            .enumerate()
            .map(|(i, row)| RankedResult {
                chunk_id: row.get("chunk_id"),
                document_id: row.get("document_id"),
                document_path: row.get("document_path"),
                content: row.get("content"),
                rank: (i + 1) as u32,
            })
            .collect())
    }
}
