//! Data volume, import stats, error handling, and extensibility tests.

use std::path::PathBuf;
use tempfile::TempDir;

use axinite::import::openclaw::reader::OpenClawReader;
use axinite::import::{ImportOptions, ImportStats};

use super::harness::setup_full_openclaw_test_env;

#[test]
fn test_full_workspace_import_counts() {
    let (_temp, openclaw_path) = setup_full_openclaw_test_env().expect("setup failed");

    let reader = OpenClawReader::new(&openclaw_path).expect("reader creation failed");

    // Count workspace files
    let workspace_count = reader
        .list_workspace_files()
        .expect("list workspace files failed");
    assert_eq!(workspace_count, 3); // MEMORY.md, README.md, AGENTS.md

    // Count agent databases
    let agent_dbs = reader.list_agent_dbs().expect("list agent dbs failed");
    assert_eq!(agent_dbs.len(), 2); // primary + secondary
}

#[test]
fn test_full_memory_chunks_import() {
    let (_temp, openclaw_path) = setup_full_openclaw_test_env().expect("setup failed");

    let reader = OpenClawReader::new(&openclaw_path).expect("reader creation failed");
    let agent_dbs = reader.list_agent_dbs().expect("list agent dbs failed");

    // Each agent should have 5 chunks
    for (_name, db_path) in agent_dbs {
        let chunks = reader
            .read_memory_chunks(&db_path)
            .expect("read memory chunks failed");
        assert_eq!(chunks.len(), 5);

        // Verify chunk structure
        for (i, chunk) in chunks.iter().enumerate() {
            assert_eq!(chunk.chunk_index, i as i32);
            assert!(
                chunk
                    .content
                    .contains(&format!("Content for section {}", i))
            );
        }
    }
}

#[test]
fn test_full_conversations_import() {
    let (_temp, openclaw_path) = setup_full_openclaw_test_env().expect("setup failed");

    let reader = OpenClawReader::new(&openclaw_path).expect("reader creation failed");
    let agent_dbs = reader.list_agent_dbs().expect("list agent dbs failed");

    // Each agent should have 3 conversations
    for (_name, db_path) in agent_dbs {
        let conversations = reader
            .read_conversations(&db_path)
            .expect("read conversations failed");
        assert_eq!(conversations.len(), 3);

        // Verify each conversation has messages
        for conv in conversations {
            assert_eq!(conv.messages.len(), 3); // Each has 3 messages
            assert!(!conv.channel.is_empty());

            // Verify message roles
            let roles: Vec<_> = conv.messages.iter().map(|m| m.role.as_str()).collect();
            assert!(roles.contains(&"user"));
            assert!(roles.contains(&"assistant"));
        }
    }
}

#[test]
fn test_import_options_validation() {
    let opts = ImportOptions {
        openclaw_path: PathBuf::from("/test/openclaw"),
        dry_run: true,
        re_embed: true,
        user_id: "test_user".to_string(),
    };

    assert_eq!(opts.user_id, "test_user");
    assert!(opts.dry_run);
    assert!(opts.re_embed);
}

#[test]
fn test_import_stats_calculations() {
    // Simulating a full import scenario
    let stats = ImportStats {
        // Workspace: 3 files
        documents: 3,
        // Memory: 2 agents × 5 chunks each = 10 chunks
        chunks: 10,
        // Conversations: 2 agents × 3 conversations = 6 conversations
        conversations: 6,
        // Messages: 2 agents × 3 conversations × 3 messages = 18 messages
        messages: 18,
        // Settings: LLM config + embeddings + custom = 3
        settings: 3,
        // Credentials: api_key + embeddings_key = 2
        secrets: 2,
        ..ImportStats::default()
    };

    let total = stats.total_imported();
    assert_eq!(total, 3 + 10 + 6 + 18 + 3 + 2);
    assert!(!stats.is_empty());
}

#[test]
fn test_error_on_corrupt_sqlite() {
    let temp_dir = TempDir::new().expect("temp dir creation failed");
    let openclaw_path = temp_dir.path().to_path_buf();

    // Create agents dir with corrupt SQLite file
    let agents_dir = openclaw_path.join("agents");
    std::fs::create_dir_all(&agents_dir).expect("agents dir creation failed");

    // Write garbage data as "SQLite"
    std::fs::write(
        agents_dir.join("corrupt.sqlite"),
        "this is not a sqlite file",
    )
    .expect("write failed");

    let reader = OpenClawReader::new(&openclaw_path).expect("reader creation failed");

    // Listing should succeed (file exists)
    let dbs = reader.list_agent_dbs().expect("list agent dbs failed");
    assert_eq!(dbs.len(), 1);

    // But reading should fail
    let result = reader.read_memory_chunks(&dbs[0].1);
    assert!(result.is_err());
}

#[test]
fn test_graceful_handling_missing_agents_directory() {
    let temp_dir = TempDir::new().expect("temp dir creation failed");
    let openclaw_path = temp_dir.path().to_path_buf();

    // Create config but no agents directory
    std::fs::write(
        openclaw_path.join("openclaw.json"),
        r#"{ llm: { provider: "openai" } }"#,
    )
    .expect("write failed");

    let reader = OpenClawReader::new(&openclaw_path).expect("reader creation failed");

    // Should return empty list, not error
    let dbs = reader.list_agent_dbs().expect("list agent dbs failed");
    assert_eq!(dbs.len(), 0);
}

#[test]
fn test_multiple_agents_independent_data() {
    let (_temp, openclaw_path) = setup_full_openclaw_test_env().expect("setup failed");

    let reader = OpenClawReader::new(&openclaw_path).expect("reader creation failed");
    let agent_dbs = reader.list_agent_dbs().expect("list agent dbs failed");

    // Verify each agent has independent data
    assert_eq!(agent_dbs.len(), 2);
    assert_eq!(agent_dbs[0].0, "primary_agent");
    assert_eq!(agent_dbs[1].0, "secondary_agent");

    // Each should have its own chunks
    for (_name, db_path) in &agent_dbs {
        let chunks = reader
            .read_memory_chunks(db_path)
            .expect("read chunks failed");
        assert_eq!(chunks.len(), 5);
    }
}

#[test]
fn test_channel_diversity_in_conversations() {
    let (_temp, openclaw_path) = setup_full_openclaw_test_env().expect("setup failed");

    let reader = OpenClawReader::new(&openclaw_path).expect("reader creation failed");
    let agent_dbs = reader.list_agent_dbs().expect("list agent dbs failed");

    // Get conversations from first agent
    let conversations = reader
        .read_conversations(&agent_dbs[0].1)
        .expect("read conversations failed");

    // Should have different channels
    let channels: std::collections::HashSet<_> =
        conversations.iter().map(|c| c.channel.as_str()).collect();
    assert!(channels.contains("telegram"));
    assert!(channels.contains("slack"));
    assert!(channels.contains("discord"));
}
