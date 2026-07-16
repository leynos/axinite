//! Hybrid, full-text, and vector search operations for the workspace repository.

use pgvector::Vector;
use uuid::Uuid;

use crate::db::HybridSearchParams;
use crate::error::WorkspaceError;
use crate::workspace::search::{RankedResult, SearchResult, reciprocal_rank_fusion};

use super::Repository;

/// Ownership scope and result cap shared by the FTS and vector searches.
struct SearchScope<'a> {
    /// Owning user whose documents are searched.
    user_id: &'a str,
    /// Optional agent the documents are scoped to.
    agent_id: Option<Uuid>,
    /// Maximum number of ranked results to return (pre-fusion limit).
    limit: usize,
}

impl Repository {
    /// Perform hybrid search combining FTS and vector similarity.
    pub async fn hybrid_search(
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

        let scope = SearchScope {
            user_id,
            agent_id,
            limit: config.pre_fusion_limit,
        };

        let fts_results = if config.use_fts {
            let results = self.fts_search(&scope, query).await?;
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
                let results = self.vector_search(&scope, embedding).await?;
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
        scope: &SearchScope<'_>,
        query: &str,
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
                &[
                    &scope.user_id,
                    &scope.agent_id,
                    &query,
                    &(scope.limit as i64),
                ],
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
        scope: &SearchScope<'_>,
        embedding: &[f32],
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
                &[
                    &scope.user_id,
                    &scope.agent_id,
                    &embedding_vec,
                    &(scope.limit as i64),
                ],
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
