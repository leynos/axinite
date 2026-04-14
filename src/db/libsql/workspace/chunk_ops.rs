//! Chunk-oriented workspace-store helpers for the libSQL backend.

use libsql::params;
use uuid::Uuid;

use super::super::{LibSqlBackend, get_i64, get_text, get_ts};
use crate::db::InsertChunkParams;
use crate::error::WorkspaceError;
use crate::workspace::MemoryChunk;

pub(super) async fn delete_chunks(
    backend: &LibSqlBackend,
    document_id: Uuid,
) -> Result<(), WorkspaceError> {
    let conn = backend
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

pub(super) async fn insert_chunk(
    backend: &LibSqlBackend,
    params: InsertChunkParams<'_>,
) -> Result<Uuid, WorkspaceError> {
    let InsertChunkParams {
        document_id,
        chunk_index,
        content,
        embedding,
    } = params;
    let conn = backend
        .connect()
        .await
        .map_err(|e| WorkspaceError::ChunkingFailed {
            reason: e.to_string(),
        })?;
    let id = Uuid::new_v4();
    let chunk_index = i64::from(chunk_index);
    let embedding_blob = embedding.and_then(|values| {
        (!values.is_empty()).then(|| {
            values
                .iter()
                .flat_map(|f| f.to_le_bytes())
                .collect::<Vec<u8>>()
        })
    });
    let embedding_value = embedding_blob
        .map(libsql::Value::Blob)
        .unwrap_or(libsql::Value::Null);

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
            embedding_value,
        ],
    )
    .await
    .map_err(|e| WorkspaceError::ChunkingFailed {
        reason: format!("Insert failed: {}", e),
    })?;
    Ok(id)
}

pub(super) async fn update_chunk_embedding(
    backend: &LibSqlBackend,
    chunk_id: Uuid,
    embedding: &[f32],
) -> Result<(), WorkspaceError> {
    let conn = backend
        .connect()
        .await
        .map_err(|e| WorkspaceError::EmbeddingFailed {
            reason: e.to_string(),
        })?;
    let embedding_value = if embedding.is_empty() {
        libsql::Value::Null
    } else {
        let bytes: Vec<u8> = embedding.iter().flat_map(|f| f.to_le_bytes()).collect();
        libsql::Value::Blob(bytes)
    };

    conn.execute(
        "UPDATE memory_chunks SET embedding = ?2 WHERE id = ?1",
        params![chunk_id.to_string(), embedding_value],
    )
    .await
    .map_err(|e| WorkspaceError::EmbeddingFailed {
        reason: format!("Update failed: {}", e),
    })?;
    Ok(())
}

pub(super) async fn get_chunks_without_embeddings(
    backend: &LibSqlBackend,
    user_id: &str,
    agent_id: Option<Uuid>,
    limit: usize,
) -> Result<Vec<MemoryChunk>, WorkspaceError> {
    let conn = backend
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
