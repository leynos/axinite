//! OpenClaw memory chunk import.

use std::sync::Arc;

use crate::db::{Database, InsertChunkParams};
use crate::import::{ImportError, ImportOptions};

use super::reader::OpenClawMemoryChunk;

/// Import a single memory chunk into IronClaw.
pub async fn import_chunk(
    db: &Arc<dyn Database>,
    chunk: &OpenClawMemoryChunk,
    opts: &ImportOptions,
) -> Result<(), ImportError> {
    // Get or create document by path
    let doc = db
        .get_or_create_document_by_path(&opts.user_id, None, &chunk.path)
        .await
        .map_err(|e| ImportError::Database(e.to_string()))?;

    // Insert chunk
    let chunk_id = db
        .insert_chunk(InsertChunkParams {
            document_id: doc.id,
            chunk_index: u32::try_from(chunk.chunk_index).map_err(|_| {
                ImportError::Database("chunk_index must be non-negative".to_string())
            })?,
            content: &chunk.content,
            embedding: None, // Don't set embedding yet if dimensions might not match
        })
        .await
        .map_err(|e| ImportError::Database(e.to_string()))?;

    // If we have an embedding, try to update it
    if let Some(ref embedding) = chunk.embedding {
        // Note: dimension check would go here if we had target dimensions available
        // For now, just store what we have
        db.update_chunk_embedding(chunk_id, embedding)
            .await
            .map_err(|e| ImportError::Database(e.to_string()))?;
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_memory_chunk_import_structure() {
        // Verify that OpenClawMemoryChunk can be created with test data
        let chunk = OpenClawMemoryChunk {
            path: "test/path.md".to_string(),
            content: "Test content".to_string(),
            embedding: Some(vec![0.1, 0.2, 0.3]),
            chunk_index: 0,
        };

        assert_eq!(chunk.path, "test/path.md");
        assert_eq!(chunk.chunk_index, 0);
        assert!(chunk.embedding.is_some());
    }

    #[cfg(feature = "libsql")]
    #[tokio::test]
    async fn import_chunk_rejects_negative_chunk_index() {
        use std::sync::Arc;

        use crate::db::libsql::LibSqlBackend;
        use crate::db::{Database, NativeDatabase};

        let backend = LibSqlBackend::new_memory()
            .await
            .expect("libsql backend should be created");
        NativeDatabase::run_migrations(&backend)
            .await
            .expect("libsql migrations should succeed");
        let db: Arc<dyn Database> = Arc::new(backend);
        let chunk = OpenClawMemoryChunk {
            path: "test/path.md".to_string(),
            content: "Test content".to_string(),
            embedding: None,
            chunk_index: -1,
        };
        let opts = ImportOptions {
            openclaw_path: std::path::PathBuf::new(),
            dry_run: false,
            re_embed: false,
            user_id: "test-user".to_string(),
        };

        let result = import_chunk(&db, &chunk, &opts).await;
        assert!(matches!(
            result,
            Err(ImportError::Database(message)) if message == "chunk_index must be non-negative"
        ));
    }
}
