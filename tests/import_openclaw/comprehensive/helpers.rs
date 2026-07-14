//! Helpers that build synthetic OpenClaw directory structures and SQLite
//! memory databases for the comprehensive import tests.

use std::path::{Path, PathBuf};
use tempfile::TempDir;
use uuid::Uuid;

/// Helper to create a minimal synthetic OpenClaw directory structure
pub(super) fn create_synthetic_openclaw_dir()
-> Result<(TempDir, PathBuf), Box<dyn std::error::Error>> {
    let temp_dir = TempDir::new()?;
    let openclaw_path = temp_dir.path().to_path_buf();

    // Create openclaw.json
    let config_content = r#"{
        llm: {
            provider: "openai",
            model: "gpt-4",
            api_key: "sk-test-key-123",
            base_url: "https://api.openai.com/v1"
        },
        embeddings: {
            model: "text-embedding-3-small",
            provider: "openai",
            api_key: "sk-test-embed-456"
        }
    }"#;
    std::fs::write(openclaw_path.join("openclaw.json"), config_content)?;

    // Create workspace directory with Markdown files
    let workspace_dir = openclaw_path.join("workspace");
    std::fs::create_dir_all(&workspace_dir)?;

    let memory_content =
        "# Memory\n\nThis is a test memory document.\n\n## Section 1\nSome content here.";
    std::fs::write(workspace_dir.join("MEMORY.md"), memory_content)?;

    let readme_content = "# README\n\nTest workspace README with important notes.";
    std::fs::write(workspace_dir.join("README.md"), readme_content)?;

    Ok((temp_dir, openclaw_path))
}

/// Helper to create a synthetic SQLite database with memory chunks
pub(super) fn create_synthetic_memory_db(
    agents_dir: &Path,
) -> Result<PathBuf, Box<dyn std::error::Error>> {
    use rusqlite::Connection;

    std::fs::create_dir_all(agents_dir)?;
    let db_path = agents_dir.join("test_agent.sqlite");

    let conn = Connection::open(&db_path)?;

    // Create chunks table (simplified schema)
    conn.execute(
        "CREATE TABLE IF NOT EXISTS chunks (
            id TEXT PRIMARY KEY,
            path TEXT NOT NULL,
            content TEXT NOT NULL,
            embedding BLOB,
            chunk_index INTEGER NOT NULL
        )",
        [],
    )?;

    // Insert test chunks
    conn.execute(
        "INSERT INTO chunks (id, path, content, embedding, chunk_index)
         VALUES (?, ?, ?, ?, ?)",
        rusqlite::params![
            Uuid::new_v4().to_string(),
            "test/doc.md",
            "This is test chunk 1 content.",
            None::<Vec<u8>>,
            0
        ],
    )?;

    conn.execute(
        "INSERT INTO chunks (id, path, content, embedding, chunk_index)
         VALUES (?, ?, ?, ?, ?)",
        rusqlite::params![
            Uuid::new_v4().to_string(),
            "test/doc.md",
            "This is test chunk 2 content.",
            None::<Vec<u8>>,
            1
        ],
    )?;

    // Create conversation table
    conn.execute(
        "CREATE TABLE IF NOT EXISTS conversations (
            id TEXT PRIMARY KEY,
            channel TEXT NOT NULL,
            created_at TEXT
        )",
        [],
    )?;

    // Create messages table
    conn.execute(
        "CREATE TABLE IF NOT EXISTS messages (
            id TEXT PRIMARY KEY,
            conversation_id TEXT NOT NULL,
            role TEXT NOT NULL,
            content TEXT NOT NULL,
            created_at TEXT,
            FOREIGN KEY(conversation_id) REFERENCES conversations(id)
        )",
        [],
    )?;

    // Insert test conversation
    let conv_id = Uuid::new_v4().to_string();
    conn.execute(
        "INSERT INTO conversations (id, channel, created_at) VALUES (?, ?, ?)",
        rusqlite::params![&conv_id, "telegram", "2024-01-15T10:30:00Z"],
    )?;

    // Insert test messages
    conn.execute(
        "INSERT INTO messages (id, conversation_id, role, content, created_at)
         VALUES (?, ?, ?, ?, ?)",
        rusqlite::params![
            Uuid::new_v4().to_string(),
            &conv_id,
            "user",
            "Hello, how are you?",
            "2024-01-15T10:30:00Z"
        ],
    )?;

    conn.execute(
        "INSERT INTO messages (id, conversation_id, role, content, created_at)
         VALUES (?, ?, ?, ?, ?)",
        rusqlite::params![
            Uuid::new_v4().to_string(),
            &conv_id,
            "assistant",
            "I'm doing well, thank you for asking!",
            "2024-01-15T10:31:00Z"
        ],
    )?;

    Ok(db_path)
}
