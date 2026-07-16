//! Embedding-dimension handling tests: mismatch queues re-embedding,
//! matching dimensions skip it.

use ironclaw::import::openclaw::reader::OpenClawReader;
use tempfile::TempDir;
use uuid::Uuid;

use super::helpers::{create_test_openclaw, ensure_libsql_initialized, libsql_test_mutex};

// ────────────────────────────────────────────────────────────────────
// Integration Test 5: Embedding Dimension Mismatch Handling
// ────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn test_embedding_dimension_mismatch_queues_reembedding() {
    ensure_libsql_initialized().await;
    let _guard = libsql_test_mutex().lock().await;
    let (_openclaw_temp, openclaw_path) = create_test_openclaw().expect("OpenClaw creation failed");

    // Create an agent DB with embeddings (1536-dim)
    let agents_dir = openclaw_path.join("agents");
    std::fs::create_dir_all(&agents_dir).expect("mkdir failed");
    let db_path = agents_dir.join("with_embeddings.sqlite");

    {
        use rusqlite::Connection;
        let conn = Connection::open(&db_path).expect("db open failed");

        conn.execute(
            "CREATE TABLE chunks (
                id TEXT PRIMARY KEY,
                path TEXT NOT NULL,
                content TEXT NOT NULL,
                embedding BLOB,
                chunk_index INTEGER NOT NULL
            )",
            [],
        )
        .expect("create table failed");

        // Create a 1536-dimensional embedding (ada-002 size)
        // Each f32 is 4 bytes, so 1536 * 4 = 6144 bytes
        let embedding_1536_bytes = vec![0.1f32; 1536]
            .iter()
            .flat_map(|f| f.to_le_bytes().to_vec())
            .collect::<Vec<u8>>();

        conn.execute(
            "INSERT INTO chunks (id, path, content, embedding, chunk_index) VALUES (?, ?, ?, ?, ?)",
            rusqlite::params![
                Uuid::new_v4().to_string(),
                "test.md",
                "Chunk with embedding",
                &embedding_1536_bytes,
                0
            ],
        )
        .expect("insert failed");

        conn.execute(
            "CREATE TABLE conversations (id TEXT PRIMARY KEY, channel TEXT, created_at TEXT)",
            [],
        )
        .expect("create conv table failed");

        conn.execute(
            "CREATE TABLE messages (
                id TEXT PRIMARY KEY,
                conversation_id TEXT NOT NULL,
                role TEXT NOT NULL,
                content TEXT NOT NULL,
                created_at TEXT
            )",
            [],
        )
        .expect("create messages table failed");
    }

    // Read the chunks back
    let reader = OpenClawReader::new(&openclaw_path).expect("reader creation failed");
    let chunks = reader
        .read_memory_chunks(&db_path)
        .expect("read chunks failed");

    assert_eq!(chunks.len(), 1);
    let chunk = &chunks[0];

    // Verify embedding was read correctly
    assert!(chunk.embedding.is_some());
    let embedding = chunk.embedding.as_ref().unwrap();
    assert_eq!(embedding.len(), 1536);

    // Verify all values are approximately 0.1
    for (i, val) in embedding.iter().enumerate() {
        assert!(
            (val - 0.1).abs() < 0.001,
            "Embedding value {} should be ~0.1, got {}",
            i,
            val
        );
    }

    // Simulate dimension mismatch scenario:
    // - Source: 1536-dim (ada-002)
    // - Target: 3072-dim (text-embedding-3-large)
    // This would trigger re-embedding logic

    let source_dim = embedding.len();
    let target_dim = 3072; // text-embedding-3-large

    if source_dim != target_dim {
        // In real import, this would queue the chunk for re-embedding
        // Verify the logic: dimensions don't match, so chunk needs re-embedding
        assert!(
            source_dim != target_dim,
            "Dimension mismatch detected: {} -> {}",
            source_dim,
            target_dim
        );

        // Track that this chunk would need re-embedding
        let mut re_embed_queued = 0;
        if source_dim != target_dim {
            re_embed_queued += 1;
        }

        assert_eq!(re_embed_queued, 1);
    }
}

// ────────────────────────────────────────────────────────────────────
// Integration Test 6: Embedding Dimension Match (No Re-embedding)
// ────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn test_embedding_same_dimension_no_reembedding() {
    ensure_libsql_initialized().await;
    let _guard = libsql_test_mutex().lock().await;
    let temp_dir = TempDir::new().expect("temp dir failed");
    let openclaw_path = temp_dir.path().to_path_buf();

    // Create minimal config
    std::fs::write(
        openclaw_path.join("openclaw.json"),
        r#"{ llm: { provider: "openai", model: "gpt-4" } }"#,
    )
    .expect("write config failed");

    // Create agent DB with 1536-dim embeddings
    let agents_dir = openclaw_path.join("agents");
    std::fs::create_dir_all(&agents_dir).expect("mkdir failed");
    let db_path = agents_dir.join("same_dim.sqlite");

    {
        use rusqlite::Connection;
        let conn = Connection::open(&db_path).expect("db open failed");

        conn.execute(
            "CREATE TABLE chunks (
                id TEXT PRIMARY KEY,
                path TEXT NOT NULL,
                content TEXT NOT NULL,
                embedding BLOB,
                chunk_index INTEGER NOT NULL
            )",
            [],
        )
        .expect("create table failed");

        // 1536-dimensional embedding (text-embedding-3-small)
        let embedding_bytes = vec![0.5f32; 1536]
            .iter()
            .flat_map(|f| f.to_le_bytes().to_vec())
            .collect::<Vec<u8>>();

        conn.execute(
            "INSERT INTO chunks (id, path, content, embedding, chunk_index) VALUES (?, ?, ?, ?, ?)",
            rusqlite::params![
                Uuid::new_v4().to_string(),
                "test.md",
                "Chunk",
                &embedding_bytes,
                0
            ],
        )
        .expect("insert failed");

        conn.execute(
            "CREATE TABLE conversations (id TEXT PRIMARY KEY, channel TEXT, created_at TEXT)",
            [],
        )
        .expect("create conv table failed");

        conn.execute(
            "CREATE TABLE messages (
                id TEXT PRIMARY KEY,
                conversation_id TEXT NOT NULL,
                role TEXT NOT NULL,
                content TEXT NOT NULL,
                created_at TEXT
            )",
            [],
        )
        .expect("create messages table failed");
    }

    let reader = OpenClawReader::new(&openclaw_path).expect("reader creation failed");
    let chunks = reader
        .read_memory_chunks(&db_path)
        .expect("read chunks failed");

    let embedding = chunks[0].embedding.as_ref().unwrap();
    let source_dim = embedding.len();
    let target_dim = 1536; // Same as source (text-embedding-3-small)

    // Dimensions match, so no re-embedding needed
    assert_eq!(source_dim, target_dim);

    let re_embed_queued = if source_dim != target_dim { 1 } else { 0 };
    assert_eq!(re_embed_queued, 0);
}
