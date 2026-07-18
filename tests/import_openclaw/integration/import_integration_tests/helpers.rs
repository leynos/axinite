//! Shared fixtures: libSQL serialization guard, test database setup,
//! and synthetic OpenClaw directory/agent-database builders.

use std::path::PathBuf;
use std::sync::{Arc, OnceLock};

use axinite::db::Database;
use axinite::db::libsql::LibSqlBackend;
use tempfile::TempDir;
use tokio::sync::{Mutex, OnceCell};
use uuid::Uuid;

pub(super) fn libsql_test_mutex() -> &'static Mutex<()> {
    static LIBSQL_TEST_MUTEX: OnceLock<Mutex<()>> = OnceLock::new();
    LIBSQL_TEST_MUTEX.get_or_init(|| Mutex::new(()))
}

pub(super) async fn ensure_libsql_initialized() {
    static LIBSQL_INIT: OnceCell<()> = OnceCell::const_new();
    LIBSQL_INIT
        .get_or_init(|| async {
            let temp_dir = TempDir::new().expect("temp dir failed");
            let db_path = temp_dir.path().join("libsql-init.db");
            let backend = LibSqlBackend::new_local(&db_path)
                .await
                .expect("libsql init failed");
            backend
                .run_migrations()
                .await
                .expect("libsql migration failed");
        })
        .await;
}

/// Helper: Create a test database and return both the DB and temp dir
pub(super) async fn create_test_db()
-> Result<(Arc<dyn axinite::db::Database>, TempDir), Box<dyn std::error::Error>> {
    let temp_dir = TempDir::new()?;
    let db_path = temp_dir.path().join("test.db");
    let backend = LibSqlBackend::new_local(&db_path).await?;
    backend.run_migrations().await?;
    let db: Arc<dyn axinite::db::Database> = Arc::new(backend);
    Ok((db, temp_dir))
}

/// Helper: Create a test OpenClaw directory with full structure
pub(super) fn create_test_openclaw() -> Result<(TempDir, PathBuf), Box<dyn std::error::Error>> {
    let temp_dir = TempDir::new()?;
    let openclaw_path = temp_dir.path().to_path_buf();

    // Config
    let config = r#"{
        llm: {
            provider: "openai",
            model: "gpt-4",
            api_key: "sk-test-12345"
        },
        embeddings: {
            model: "text-embedding-3-small",
            api_key: "sk-embed-67890"
        }
    }"#;
    std::fs::write(openclaw_path.join("openclaw.json"), config)?;

    // Workspace files
    let workspace_dir = openclaw_path.join("workspace");
    std::fs::create_dir_all(&workspace_dir)?;
    std::fs::write(
        workspace_dir.join("MEMORY.md"),
        "# Memory\n\nTest memory content for integration test.",
    )?;
    std::fs::write(
        workspace_dir.join("NOTES.md"),
        "# Notes\n\nAdditional notes content.",
    )?;

    // Agent databases
    let agents_dir = openclaw_path.join("agents");
    std::fs::create_dir_all(&agents_dir)?;

    create_test_agent_db(&agents_dir.join("agent1.sqlite"))?;
    create_test_agent_db(&agents_dir.join("agent2.sqlite"))?;

    Ok((temp_dir, openclaw_path))
}

/// Helper: Create a test agent SQLite database
fn create_test_agent_db(db_path: &PathBuf) -> Result<(), Box<dyn std::error::Error>> {
    use rusqlite::Connection;

    let conn = Connection::open(db_path)?;

    // Chunks table
    conn.execute(
        "CREATE TABLE chunks (
            id TEXT PRIMARY KEY,
            path TEXT NOT NULL,
            content TEXT NOT NULL,
            embedding BLOB,
            chunk_index INTEGER NOT NULL
        )",
        [],
    )?;

    for i in 0..3 {
        conn.execute(
            "INSERT INTO chunks (id, path, content, embedding, chunk_index) VALUES (?, ?, ?, ?, ?)",
            rusqlite::params![
                Uuid::new_v4().to_string(),
                format!("doc/section_{}.md", i),
                format!("Chunk {} content", i),
                None::<Vec<u8>>,
                i
            ],
        )?;
    }

    // Conversations
    conn.execute(
        "CREATE TABLE conversations (id TEXT PRIMARY KEY, channel TEXT, created_at TEXT)",
        [],
    )?;

    conn.execute(
        "CREATE TABLE messages (
            id TEXT PRIMARY KEY,
            conversation_id TEXT NOT NULL,
            role TEXT NOT NULL,
            content TEXT NOT NULL,
            created_at TEXT
        )",
        [],
    )?;

    let conv_id = Uuid::new_v4().to_string();
    conn.execute(
        "INSERT INTO conversations VALUES (?, ?, ?)",
        rusqlite::params![&conv_id, "slack", "2024-01-15T10:00:00Z"],
    )?;

    for j in 0..2 {
        conn.execute(
            "INSERT INTO messages (id, conversation_id, role, content, created_at) VALUES (?, ?, ?, ?, ?)",
            rusqlite::params![
                Uuid::new_v4().to_string(),
                &conv_id,
                if j % 2 == 0 { "user" } else { "assistant" },
                format!("Message {}", j),
                format!("2024-01-15T10:{:02}:00Z", j)
            ],
        )?;
    }

    Ok(())
}
