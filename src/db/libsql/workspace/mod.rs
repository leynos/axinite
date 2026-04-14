//! Workspace-store operations for the libSQL backend.

mod chunk_ops;
mod document_ops;
mod fts;
#[cfg(test)]
mod tests;
mod vector_search;

use uuid::Uuid;

use super::LibSqlBackend;
use crate::db::{HybridSearchParams, InsertChunkParams, NativeWorkspaceStore};
use crate::error::WorkspaceError;
use crate::workspace::{
    MemoryChunk, MemoryDocument, SearchResult, WorkspaceEntry, reciprocal_rank_fusion,
};
use chunk_ops::{
    delete_chunks, get_chunks_without_embeddings, insert_chunk, update_chunk_embedding,
};
use document_ops::{
    AgentScope, delete_document_by_path, get_document_by_id, get_document_by_path,
    get_or_create_document_by_path, list_all_paths, list_directory, list_documents,
    update_document,
};
use fts::{FtsSearchParams, fts_ranked_results};
use vector_search::{
    VectorIndexQuery, VectorSearchOutcome, VectorSearchQuery, vector_ranked_results,
};

impl NativeWorkspaceStore for LibSqlBackend {
    async fn get_document_by_path(
        &self,
        user_id: &str,
        agent_id: Option<Uuid>,
        path: &str,
    ) -> Result<MemoryDocument, WorkspaceError> {
        get_document_by_path(self, &AgentScope { user_id, agent_id }, path).await
    }

    async fn get_document_by_id(&self, id: Uuid) -> Result<MemoryDocument, WorkspaceError> {
        get_document_by_id(self, id).await
    }

    async fn get_or_create_document_by_path(
        &self,
        user_id: &str,
        agent_id: Option<Uuid>,
        path: &str,
    ) -> Result<MemoryDocument, WorkspaceError> {
        get_or_create_document_by_path(self, &AgentScope { user_id, agent_id }, path).await
    }

    async fn update_document(&self, id: Uuid, content: &str) -> Result<(), WorkspaceError> {
        update_document(self, id, content).await
    }

    async fn delete_document_by_path(
        &self,
        user_id: &str,
        agent_id: Option<Uuid>,
        path: &str,
    ) -> Result<(), WorkspaceError> {
        delete_document_by_path(self, &AgentScope { user_id, agent_id }, path).await
    }

    async fn list_directory(
        &self,
        user_id: &str,
        agent_id: Option<Uuid>,
        directory: &str,
    ) -> Result<Vec<WorkspaceEntry>, WorkspaceError> {
        list_directory(self, &AgentScope { user_id, agent_id }, directory).await
    }

    async fn list_all_paths(
        &self,
        user_id: &str,
        agent_id: Option<Uuid>,
    ) -> Result<Vec<String>, WorkspaceError> {
        list_all_paths(self, &AgentScope { user_id, agent_id }).await
    }

    async fn list_documents(
        &self,
        user_id: &str,
        agent_id: Option<Uuid>,
    ) -> Result<Vec<MemoryDocument>, WorkspaceError> {
        list_documents(self, &AgentScope { user_id, agent_id }).await
    }

    async fn delete_chunks(&self, document_id: Uuid) -> Result<(), WorkspaceError> {
        delete_chunks(self, document_id).await
    }

    async fn insert_chunk(&self, params: InsertChunkParams<'_>) -> Result<Uuid, WorkspaceError> {
        insert_chunk(self, params).await
    }

    async fn update_chunk_embedding(
        &self,
        chunk_id: Uuid,
        embedding: &[f32],
    ) -> Result<(), WorkspaceError> {
        update_chunk_embedding(self, chunk_id, embedding).await
    }

    async fn get_chunks_without_embeddings(
        &self,
        user_id: &str,
        agent_id: Option<Uuid>,
        limit: usize,
    ) -> Result<Vec<MemoryChunk>, WorkspaceError> {
        get_chunks_without_embeddings(self, user_id, agent_id, limit).await
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
            let results = fts_ranked_results(
                &conn,
                FtsSearchParams {
                    user_id,
                    agent_id: agent_id_str.as_deref(),
                    query,
                    limit: pre_limit,
                },
            )
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
                match vector_ranked_results(
                    &conn,
                    &VectorIndexQuery {
                        user_id,
                        agent_id: agent_id_str.as_deref(),
                        embedding: emb,
                        limit: pre_limit,
                    },
                )
                .await?
                {
                    VectorSearchOutcome::Indexed(results) => results,
                    VectorSearchOutcome::IndexUnavailable => {
                        tracing::info!("Using brute-force vector search (no vector index)");
                        self.brute_force_vector_search(
                            VectorSearchQuery {
                                user_id,
                                agent_id,
                                embedding: emb,
                            },
                            pre_limit as usize,
                        )
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
