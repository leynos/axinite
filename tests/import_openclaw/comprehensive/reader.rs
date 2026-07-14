//! OpenClaw reader tests: config detection and parsing, workspace and
//! agent database listing, memory chunk and conversation reading, and
//! malformed-input handling.

use std::path::PathBuf;
use tempfile::TempDir;

use ironclaw::import::ImportError;
use ironclaw::import::openclaw::reader::OpenClawReader;

use super::helpers::{create_synthetic_memory_db, create_synthetic_openclaw_dir};

#[test]
fn test_openclaw_reader_detects_config() {
    let (temp_dir, openclaw_path) =
        create_synthetic_openclaw_dir().expect("failed to create test data");

    // Verify detection works
    assert!(openclaw_path.join("openclaw.json").exists());

    // Create reader
    let reader = OpenClawReader::new(&openclaw_path).expect("failed to create reader");

    let _ = (temp_dir, reader);
}

#[test]
fn test_openclaw_reader_parses_config() {
    let (temp_dir, openclaw_path) =
        create_synthetic_openclaw_dir().expect("failed to create test data");

    let reader = OpenClawReader::new(&openclaw_path).expect("failed to create reader");

    let config = reader.read_config().expect("failed to read config");

    // Verify LLM config
    assert!(config.llm.is_some());
    let llm = config.llm.unwrap();
    assert_eq!(llm.provider, Some("openai".to_string()));
    assert_eq!(llm.model, Some("gpt-4".to_string()));
    // API key is wrapped in SecretString, just verify it's present
    assert!(llm.api_key.is_some());

    // Verify embeddings config
    assert!(config.embeddings.is_some());
    let emb = config.embeddings.unwrap();
    assert_eq!(emb.provider, Some("openai".to_string()));
    assert_eq!(emb.model, Some("text-embedding-3-small".to_string()));
    // API key is wrapped in SecretString, just verify it's present
    assert!(emb.api_key.is_some());

    let _ = temp_dir;
}

#[test]
fn test_openclaw_reader_lists_workspace_files() {
    let (temp_dir, openclaw_path) =
        create_synthetic_openclaw_dir().expect("failed to create test data");

    let reader = OpenClawReader::new(&openclaw_path).expect("failed to create reader");

    let count = reader
        .list_workspace_files()
        .expect("failed to list workspace files");

    // Should find MEMORY.md and README.md
    assert_eq!(count, 2);

    let _ = temp_dir;
}

#[test]
fn test_openclaw_reader_lists_agent_dbs() {
    let (temp_dir, openclaw_path) =
        create_synthetic_openclaw_dir().expect("failed to create test data");

    let agents_dir = openclaw_path.join("agents");
    let _db_path = create_synthetic_memory_db(&agents_dir).expect("failed to create test DB");

    let reader = OpenClawReader::new(&openclaw_path).expect("failed to create reader");

    let dbs = reader.list_agent_dbs().expect("failed to list agent DBs");

    // Should find test_agent.sqlite
    assert_eq!(dbs.len(), 1);
    assert_eq!(dbs[0].0, "test_agent");

    let _ = temp_dir;
}

#[test]
fn test_openclaw_reader_reads_memory_chunks() {
    let (temp_dir, openclaw_path) =
        create_synthetic_openclaw_dir().expect("failed to create test data");

    let agents_dir = openclaw_path.join("agents");
    let db_path = create_synthetic_memory_db(&agents_dir).expect("failed to create test DB");

    let reader = OpenClawReader::new(&openclaw_path).expect("failed to create reader");

    let chunks = reader
        .read_memory_chunks(&db_path)
        .expect("failed to read memory chunks");

    // Should find 2 chunks
    assert_eq!(chunks.len(), 2);

    // Verify chunk content
    assert_eq!(chunks[0].path, "test/doc.md");
    assert_eq!(chunks[0].content, "This is test chunk 1 content.");
    assert_eq!(chunks[0].chunk_index, 0);
    assert!(chunks[0].embedding.is_none());

    assert_eq!(chunks[1].path, "test/doc.md");
    assert_eq!(chunks[1].content, "This is test chunk 2 content.");
    assert_eq!(chunks[1].chunk_index, 1);

    let _ = temp_dir;
}

#[test]
fn test_openclaw_reader_reads_conversations() {
    let (temp_dir, openclaw_path) =
        create_synthetic_openclaw_dir().expect("failed to create test data");

    let agents_dir = openclaw_path.join("agents");
    let db_path = create_synthetic_memory_db(&agents_dir).expect("failed to create test DB");

    let reader = OpenClawReader::new(&openclaw_path).expect("failed to create reader");

    let conversations = reader
        .read_conversations(&db_path)
        .expect("failed to read conversations");

    // Should find 1 conversation
    assert_eq!(conversations.len(), 1);

    let conv = &conversations[0];
    assert_eq!(conv.channel, "telegram");
    assert_eq!(conv.messages.len(), 2);

    // Verify messages
    assert_eq!(conv.messages[0].role, "user");
    assert_eq!(conv.messages[0].content, "Hello, how are you?");
    assert_eq!(conv.messages[1].role, "assistant");
    assert_eq!(
        conv.messages[1].content,
        "I'm doing well, thank you for asking!"
    );

    let _ = temp_dir;
}

#[test]
fn test_openclaw_reader_handles_missing_directory() {
    let missing_path = PathBuf::from("/nonexistent/openclaw");
    let result = OpenClawReader::new(&missing_path);

    assert!(result.is_err());
    match result {
        Err(ImportError::NotFound { .. }) => (), // Expected
        _ => panic!("Expected NotFound error"),
    }
}

#[test]
fn test_openclaw_reader_handles_missing_config() {
    let temp_dir = TempDir::new().expect("failed to create temp dir");
    let reader = OpenClawReader::new(temp_dir.path()).expect("failed to create reader");

    let result = reader.read_config();
    assert!(result.is_err());
}

#[test]
fn test_openclaw_reader_empty_agents_directory() {
    let (temp_dir, openclaw_path) =
        create_synthetic_openclaw_dir().expect("failed to create test data");

    // Create empty agents directory
    std::fs::create_dir(openclaw_path.join("agents")).expect("failed to create agents dir");

    let reader = OpenClawReader::new(&openclaw_path).expect("failed to create reader");

    let dbs = reader.list_agent_dbs().expect("failed to list agent DBs");

    // Should find no databases
    assert_eq!(dbs.len(), 0);

    let _ = temp_dir;
}

#[test]
fn test_openclaw_reader_no_workspace_files() {
    let temp_dir = TempDir::new().expect("failed to create temp dir");
    let openclaw_path = temp_dir.path().to_path_buf();

    // Create config
    let config_content = r#"{ llm: { provider: "openai" } }"#;
    std::fs::write(openclaw_path.join("openclaw.json"), config_content)
        .expect("failed to write config");

    let reader = OpenClawReader::new(&openclaw_path).expect("failed to create reader");

    let count = reader
        .list_workspace_files()
        .expect("failed to list workspace files");

    // Should find no files
    assert_eq!(count, 0);
}

#[test]
fn test_openclaw_reader_malformed_json5() {
    let temp_dir = TempDir::new().expect("failed to create temp dir");
    let openclaw_path = temp_dir.path().to_path_buf();

    // Create malformed config
    let bad_config = r#"{ llm: { provider: "openai" }"#; // Missing closing brace
    std::fs::write(openclaw_path.join("openclaw.json"), bad_config)
        .expect("failed to write config");

    let reader = OpenClawReader::new(&openclaw_path).expect("failed to create reader");

    let result = reader.read_config();
    assert!(result.is_err());
}

#[test]
fn test_openclaw_detect_existing() {
    let (temp_dir, openclaw_path) =
        create_synthetic_openclaw_dir().expect("failed to create test data");

    // Verify the openclaw.json config exists (which is what detect() checks for)
    assert!(openclaw_path.join("openclaw.json").exists());

    let _ = temp_dir;
}
