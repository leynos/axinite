//! Synthetic OpenClaw environment builders shared by the e2e test modules.

use std::path::PathBuf;
use tempfile::TempDir;
use uuid::Uuid;

/// Helper: Create a synthetic OpenClaw with full structure
pub(super) fn setup_full_openclaw_test_env()
-> Result<(TempDir, PathBuf), Box<dyn std::error::Error>> {
    let temp_dir = TempDir::new()?;
    let openclaw_path = temp_dir.path().to_path_buf();

    // 1. Create openclaw.json with all settings
    let config_content = r#"{
            llm: {
                provider: "openai",
                model: "gpt-4-turbo",
                api_key: "sk-test-key-12345",
                base_url: "https://api.openai.com/v1"
            },
            embeddings: {
                model: "text-embedding-3-large",
                provider: "openai",
                api_key: "sk-embed-key-67890"
            },
            custom_setting: "custom_value"
        }"#;
    std::fs::write(openclaw_path.join("openclaw.json"), config_content)?;

    // 2. Create workspace with multiple files
    let workspace_dir = openclaw_path.join("workspace");
    std::fs::create_dir_all(&workspace_dir)?;

    std::fs::write(
        workspace_dir.join("MEMORY.md"),
        "# Memory\n\nStored memories and facts.\n\n- User prefers morning briefings\n- Key project: Alpha",
    )?;

    std::fs::write(
        workspace_dir.join("README.md"),
        "# Project README\n\nThis is the main project documentation.\n\n## Goals\n1. Complete migration\n2. Verify data",
    )?;

    std::fs::write(
        workspace_dir.join("AGENTS.md"),
        "# Agent Definitions\n\n## Main Agent\n- Role: Assistant\n- Capabilities: Analysis, Planning",
    )?;

    // 3. Create agents directory with databases
    let agents_dir = openclaw_path.join("agents");
    std::fs::create_dir_all(&agents_dir)?;

    create_full_agent_db(&agents_dir.join("primary_agent.sqlite"))?;
    create_full_agent_db(&agents_dir.join("secondary_agent.sqlite"))?;

    Ok((temp_dir, openclaw_path))
}

/// Helper: Create a full agent SQLite database with chunks and conversations
fn create_full_agent_db(db_path: &PathBuf) -> Result<(), Box<dyn std::error::Error>> {
    let conn = rusqlite::Connection::open(db_path)?;
    seed_chunks(&conn)?;
    create_conversation_schema(&conn)?;
    seed_conversations(&conn)?;
    Ok(())
}

/// Create the chunks table and insert five synthetic chunks.
fn seed_chunks(conn: &rusqlite::Connection) -> Result<(), Box<dyn std::error::Error>> {
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

    for i in 0..5 {
        conn.execute(
            "INSERT INTO chunks (id, path, content, embedding, chunk_index)
                 VALUES (?, ?, ?, ?, ?)",
            rusqlite::params![
                Uuid::new_v4().to_string(),
                format!("notes/section_{}.md", i),
                format!("Content for section {}. This is important information.", i),
                None::<Vec<u8>>,
                i
            ],
        )?;
    }
    Ok(())
}

/// Create the conversations and messages tables.
fn create_conversation_schema(
    conn: &rusqlite::Connection,
) -> Result<(), Box<dyn std::error::Error>> {
    conn.execute(
        "CREATE TABLE IF NOT EXISTS conversations (
                id TEXT PRIMARY KEY,
                channel TEXT NOT NULL,
                created_at TEXT
            )",
        [],
    )?;

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
    Ok(())
}

/// Insert three conversations (telegram, slack, discord), each with three
/// alternating user/assistant messages.
fn seed_conversations(conn: &rusqlite::Connection) -> Result<(), Box<dyn std::error::Error>> {
    for conv_num in 0..3 {
        let conv_id = Uuid::new_v4().to_string();
        let channel = match conv_num {
            0 => "telegram",
            1 => "slack",
            _ => "discord",
        };

        conn.execute(
            "INSERT INTO conversations (id, channel, created_at) VALUES (?, ?, ?)",
            rusqlite::params![
                &conv_id,
                channel,
                format!("2024-01-{:02}T10:00:00Z", 10 + conv_num)
            ],
        )?;

        seed_messages(conn, &conv_id, conv_num)?;
    }
    Ok(())
}

/// Insert three alternating user/assistant messages for one conversation.
fn seed_messages(
    conn: &rusqlite::Connection,
    conv_id: &str,
    conv_num: i32,
) -> Result<(), Box<dyn std::error::Error>> {
    for msg_num in 0..3 {
        let role = if msg_num % 2 == 0 {
            "user"
        } else {
            "assistant"
        };
        conn.execute(
            "INSERT INTO messages (id, conversation_id, role, content, created_at)
                 VALUES (?, ?, ?, ?, ?)",
            rusqlite::params![
                Uuid::new_v4().to_string(),
                conv_id,
                role,
                format!(
                    "{} message {} from conversation {}",
                    role, msg_num, conv_num
                ),
                format!("2024-01-{:02}T10:{:02}:00Z", 10 + conv_num, msg_num * 10)
            ],
        )?;
    }
    Ok(())
}
