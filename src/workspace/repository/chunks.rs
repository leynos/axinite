//! Chunk persistence operations for the workspace repository.

use pgvector::Vector;
use uuid::Uuid;

use crate::error::WorkspaceError;
use crate::workspace::document::MemoryChunk;

use super::Repository;

impl Repository {
    /// Delete all chunks for a document.
    pub async fn delete_chunks(&self, document_id: Uuid) -> Result<(), WorkspaceError> {
        let conn = self.conn().await?;

        conn.execute(
            "DELETE FROM memory_chunks WHERE document_id = $1",
            &[&document_id],
        )
        .await
        .map_err(|e| WorkspaceError::ChunkingFailed {
            reason: format!("Delete failed: {}", e),
        })?;

        Ok(())
    }

    /// Insert a chunk.
    pub async fn insert_chunk(
        &self,
        document_id: Uuid,
        chunk_index: u32,
        content: &str,
        embedding: Option<&[f32]>,
    ) -> Result<Uuid, WorkspaceError> {
        let conn = self.conn().await?;
        let id = Uuid::new_v4();
        let chunk_index =
            i32::try_from(chunk_index).map_err(|_| WorkspaceError::ChunkingFailed {
                reason: "chunk_index exceeds i32 range".to_string(),
            })?;

        let embedding_vec = embedding.map(|e| Vector::from(e.to_vec()));

        conn.execute(
            r#"
            INSERT INTO memory_chunks (id, document_id, chunk_index, content, embedding)
            VALUES ($1, $2, $3, $4, $5)
            "#,
            &[&id, &document_id, &chunk_index, &content, &embedding_vec],
        )
        .await
        .map_err(|e| WorkspaceError::ChunkingFailed {
            reason: format!("Insert failed: {}", e),
        })?;

        Ok(id)
    }

    /// Update a chunk's embedding.
    pub async fn update_chunk_embedding(
        &self,
        chunk_id: Uuid,
        embedding: &[f32],
    ) -> Result<(), WorkspaceError> {
        let conn = self.conn().await?;
        let embedding_vec = Vector::from(embedding.to_vec());

        conn.execute(
            "UPDATE memory_chunks SET embedding = $2 WHERE id = $1",
            &[&chunk_id, &embedding_vec],
        )
        .await
        .map_err(|e| WorkspaceError::EmbeddingFailed {
            reason: format!("Update failed: {}", e),
        })?;

        Ok(())
    }

    /// Get chunks without embeddings for backfilling.
    pub async fn get_chunks_without_embeddings(
        &self,
        user_id: &str,
        agent_id: Option<Uuid>,
        limit: usize,
    ) -> Result<Vec<MemoryChunk>, WorkspaceError> {
        let conn = self.conn().await?;

        let rows = conn
            .query(
                r#"
                SELECT c.id, c.document_id, c.chunk_index, c.content, c.created_at
                FROM memory_chunks c
                JOIN memory_documents d ON d.id = c.document_id
                WHERE d.user_id = $1 AND d.agent_id IS NOT DISTINCT FROM $2
                  AND c.embedding IS NULL
                LIMIT $3
                "#,
                &[&user_id, &agent_id, &(limit as i64)],
            )
            .await
            .map_err(|e| WorkspaceError::SearchFailed {
                reason: format!("Query failed: {}", e),
            })?;

        let mut chunks = Vec::with_capacity(rows.len());
        for row in &rows {
            let chunk_index = u32::try_from(row.get::<_, i32>("chunk_index")).map_err(|_| {
                WorkspaceError::SearchFailed {
                    reason: "memory_chunks.chunk_index must be non-negative".to_string(),
                }
            })?;
            chunks.push(MemoryChunk {
                id: row.get("id"),
                document_id: row.get("document_id"),
                chunk_index,
                content: row.get("content"),
                embedding: None,
                created_at: row.get("created_at"),
            });
        }

        Ok(chunks)
    }
}
