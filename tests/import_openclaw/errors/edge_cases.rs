//! Edge case tests for the OpenClaw importer: empty tables, very large
//! content, special characters, and NULL values.

use tempfile::TempDir;

use ironclaw::import::openclaw::reader::OpenClawReader;

// ────────────────────────────────────────────────────────────────────
// Edge Cases
// ────────────────────────────────────────────────────────────────────

#[test]
fn test_edge_case_empty_chunks_table() {
    let temp_dir = TempDir::new().expect("temp dir creation failed");
    let openclaw_path = temp_dir.path().to_path_buf();

    let agents_dir = openclaw_path.join("agents");
    std::fs::create_dir_all(&agents_dir).expect("mkdir failed");

    let db_path = agents_dir.join("empty.sqlite");

    use rusqlite::Connection;
    let conn = Connection::open(&db_path).expect("db creation failed");
    conn.execute(
        "CREATE TABLE chunks (id TEXT, path TEXT, content TEXT, embedding BLOB, chunk_index INTEGER)",
        [],
    )
    .expect("create table failed");
    drop(conn);

    let reader = OpenClawReader::new(&openclaw_path).expect("reader creation failed");

    let dbs = reader.list_agent_dbs().expect("list agent dbs failed");

    // Should succeed but return empty list
    let chunks = reader
        .read_memory_chunks(&dbs[0].1)
        .expect("read chunks failed");
    assert_eq!(chunks.len(), 0);
}

#[test]
fn test_edge_case_empty_conversations_table() {
    let temp_dir = TempDir::new().expect("temp dir creation failed");
    let openclaw_path = temp_dir.path().to_path_buf();

    let agents_dir = openclaw_path.join("agents");
    std::fs::create_dir_all(&agents_dir).expect("mkdir failed");

    let db_path = agents_dir.join("empty_conv.sqlite");

    use rusqlite::Connection;
    let conn = Connection::open(&db_path).expect("db creation failed");
    conn.execute(
        "CREATE TABLE conversations (id TEXT, channel TEXT, created_at TEXT)",
        [],
    )
    .expect("create table failed");
    conn.execute(
        "CREATE TABLE messages (id TEXT, conversation_id TEXT, role TEXT, content TEXT, created_at TEXT)",
        [],
    )
    .expect("create table failed");
    drop(conn);

    let reader = OpenClawReader::new(&openclaw_path).expect("reader creation failed");

    let dbs = reader.list_agent_dbs().expect("list agent dbs failed");

    // Should succeed but return empty list
    let conversations = reader
        .read_conversations(&dbs[0].1)
        .expect("read conversations failed");
    assert_eq!(conversations.len(), 0);
}

#[test]
fn test_edge_case_very_large_content() {
    let temp_dir = TempDir::new().expect("temp dir creation failed");
    let openclaw_path = temp_dir.path().to_path_buf();

    let agents_dir = openclaw_path.join("agents");
    std::fs::create_dir_all(&agents_dir).expect("mkdir failed");

    let db_path = agents_dir.join("large.sqlite");

    use rusqlite::Connection;
    let conn = Connection::open(&db_path).expect("db creation failed");
    conn.execute(
        "CREATE TABLE chunks (id TEXT, path TEXT, content TEXT, embedding BLOB, chunk_index INTEGER)",
        [],
    )
    .expect("create table failed");

    // Insert very large content (1MB)
    let large_content = "x".repeat(1024 * 1024);
    conn.execute(
        "INSERT INTO chunks VALUES (?, ?, ?, ?, ?)",
        rusqlite::params!["id1", "path", large_content, None::<Vec<u8>>, 0],
    )
    .expect("insert failed");
    drop(conn);

    let reader = OpenClawReader::new(&openclaw_path).expect("reader creation failed");

    let dbs = reader.list_agent_dbs().expect("list agent dbs failed");

    // Should still succeed
    let chunks = reader
        .read_memory_chunks(&dbs[0].1)
        .expect("read chunks failed");
    assert_eq!(chunks.len(), 1);
    assert_eq!(chunks[0].content.len(), 1024 * 1024);
}

#[test]
fn test_edge_case_special_characters_in_content() {
    let temp_dir = TempDir::new().expect("temp dir creation failed");
    let openclaw_path = temp_dir.path().to_path_buf();

    let agents_dir = openclaw_path.join("agents");
    std::fs::create_dir_all(&agents_dir).expect("mkdir failed");

    let db_path = agents_dir.join("special.sqlite");

    use rusqlite::Connection;
    let conn = Connection::open(&db_path).expect("db creation failed");
    conn.execute(
        "CREATE TABLE chunks (id TEXT, path TEXT, content TEXT, embedding BLOB, chunk_index INTEGER)",
        [],
    )
    .expect("create table failed");

    // Insert content with special characters
    let special_content = "Content with emoji 🚀 and UTF-8: 中文, العربية, ελληνικά";
    conn.execute(
        "INSERT INTO chunks VALUES (?, ?, ?, ?, ?)",
        rusqlite::params!["id1", "path", special_content, None::<Vec<u8>>, 0],
    )
    .expect("insert failed");
    drop(conn);

    let reader = OpenClawReader::new(&openclaw_path).expect("reader creation failed");

    let dbs = reader.list_agent_dbs().expect("list agent dbs failed");

    // Should handle special characters
    let chunks = reader
        .read_memory_chunks(&dbs[0].1)
        .expect("read chunks failed");
    assert_eq!(chunks.len(), 1);
    assert!(chunks[0].content.contains("🚀"));
    assert!(chunks[0].content.contains("中文"));
}

#[test]
fn test_edge_case_null_values_in_fields() {
    let temp_dir = TempDir::new().expect("temp dir creation failed");
    let openclaw_path = temp_dir.path().to_path_buf();

    let agents_dir = openclaw_path.join("agents");
    std::fs::create_dir_all(&agents_dir).expect("mkdir failed");

    let db_path = agents_dir.join("nulls.sqlite");

    use rusqlite::Connection;
    let conn = Connection::open(&db_path).expect("db creation failed");
    conn.execute(
        "CREATE TABLE conversations (id TEXT, channel TEXT, created_at TEXT)",
        [],
    )
    .expect("create table failed");
    conn.execute(
        "CREATE TABLE messages (id TEXT, conversation_id TEXT, role TEXT, content TEXT, created_at TEXT)",
        [],
    )
    .expect("create table failed");

    // Insert conversation with NULL created_at
    conn.execute(
        "INSERT INTO conversations VALUES (?, ?, ?)",
        rusqlite::params!["conv1", "telegram", None::<String>],
    )
    .expect("insert failed");

    // Insert message with NULL created_at
    conn.execute(
        "INSERT INTO messages VALUES (?, ?, ?, ?, ?)",
        rusqlite::params!["msg1", "conv1", "user", "hello", None::<String>],
    )
    .expect("insert failed");
    drop(conn);

    let reader = OpenClawReader::new(&openclaw_path).expect("reader creation failed");

    let dbs = reader.list_agent_dbs().expect("list agent dbs failed");

    // Should handle NULL timestamps gracefully
    let conversations = reader
        .read_conversations(&dbs[0].1)
        .expect("read conversations failed");
    assert_eq!(conversations.len(), 1);
    assert!(conversations[0].created_at.is_none());
    assert!(conversations[0].messages[0].created_at.is_none());
}
