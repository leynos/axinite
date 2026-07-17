//! SQLite database corruption and missing-table error tests for the
//! OpenClaw importer.

use tempfile::TempDir;

use axinite::import::openclaw::reader::OpenClawReader;

// ────────────────────────────────────────────────────────────────────
// SQLite Database Errors
// ────────────────────────────────────────────────────────────────────

#[test]
fn test_error_corrupt_sqlite_file() {
    let temp_dir = TempDir::new().expect("temp dir creation failed");
    let openclaw_path = temp_dir.path().to_path_buf();

    let agents_dir = openclaw_path.join("agents");
    std::fs::create_dir_all(&agents_dir).expect("mkdir failed");

    // Write invalid SQLite data
    std::fs::write(
        agents_dir.join("bad.sqlite"),
        "this is definitely not a sqlite database",
    )
    .expect("write failed");

    let reader = OpenClawReader::new(&openclaw_path).expect("reader creation failed");

    let dbs = reader.list_agent_dbs().expect("list agent dbs failed");
    assert_eq!(dbs.len(), 1);

    // But reading should fail
    let result = reader.read_memory_chunks(&dbs[0].1);
    assert!(result.is_err());
}

#[test]
fn test_error_missing_chunks_table() {
    let temp_dir = TempDir::new().expect("temp dir creation failed");
    let openclaw_path = temp_dir.path().to_path_buf();

    let agents_dir = openclaw_path.join("agents");
    std::fs::create_dir_all(&agents_dir).expect("mkdir failed");

    let db_path = agents_dir.join("no_chunks.sqlite");

    // Create valid SQLite but without chunks table
    use rusqlite::Connection;
    let conn = Connection::open(&db_path).expect("db creation failed");
    conn.execute(
        "CREATE TABLE metadata (key TEXT PRIMARY KEY, value TEXT)",
        [],
    )
    .expect("create table failed");
    drop(conn);

    let reader = OpenClawReader::new(&openclaw_path).expect("reader creation failed");

    let dbs = reader.list_agent_dbs().expect("list agent dbs failed");
    assert_eq!(dbs.len(), 1);

    // Should fail: chunks table doesn't exist
    let result = reader.read_memory_chunks(&dbs[0].1);
    assert!(result.is_err());
}

#[test]
fn test_error_missing_conversations_table() {
    let temp_dir = TempDir::new().expect("temp dir creation failed");
    let openclaw_path = temp_dir.path().to_path_buf();

    let agents_dir = openclaw_path.join("agents");
    std::fs::create_dir_all(&agents_dir).expect("mkdir failed");

    let db_path = agents_dir.join("no_conversations.sqlite");

    use rusqlite::Connection;
    let conn = Connection::open(&db_path).expect("db creation failed");
    // Only create chunks table, not conversations
    conn.execute(
        "CREATE TABLE chunks (id TEXT, path TEXT, content TEXT, embedding BLOB, chunk_index INTEGER)",
        [],
    )
    .expect("create table failed");
    drop(conn);

    let reader = OpenClawReader::new(&openclaw_path).expect("reader creation failed");

    let dbs = reader.list_agent_dbs().expect("list agent dbs failed");
    assert_eq!(dbs.len(), 1);

    // Should fail: conversations table doesn't exist
    let result = reader.read_conversations(&dbs[0].1);
    assert!(result.is_err());
}
