//! Workspace memory operations, daily log, and search tests.
//!
//! Requires a running PostgreSQL with pgvector extension.

use super::{cleanup_user, get_pool, try_connect};
use axinite::workspace::{MockEmbeddings, SearchConfig, Workspace};
use std::sync::Arc;

#[tokio::test]
async fn test_workspace_memory_operations() {
    let pool = get_pool();
    if try_connect(&pool).await.is_none() {
        return;
    }
    let user_id = "test_memory_ops";
    cleanup_user(&pool, user_id).await;

    let workspace = Workspace::new(user_id, pool.clone());

    // Append to memory
    workspace
        .append_memory("User prefers dark mode")
        .await
        .expect("Failed to append memory");
    workspace
        .append_memory("User's timezone is PST")
        .await
        .expect("Failed to append memory");

    // Read memory
    let memory = workspace.memory().await.expect("Failed to get memory");
    assert!(memory.content.contains("dark mode"));
    assert!(memory.content.contains("PST"));
    // Entries should be separated by double newline
    assert!(memory.content.contains("\n\n"));

    cleanup_user(&pool, user_id).await;
}

#[tokio::test]
async fn test_workspace_daily_log() {
    let pool = get_pool();
    if try_connect(&pool).await.is_none() {
        return;
    }
    let user_id = "test_daily_log";
    cleanup_user(&pool, user_id).await;

    let workspace = Workspace::new(user_id, pool.clone());

    // Append to daily log (timestamped)
    workspace
        .append_daily_log("Started working on feature X")
        .await
        .expect("Failed to append daily log");

    // Read today's log
    let log = workspace
        .today_log()
        .await
        .expect("Failed to get today log");
    assert!(log.content.contains("feature X"));
    // Should have timestamp prefix like [HH:MM:SS]
    assert!(log.content.contains("["));

    cleanup_user(&pool, user_id).await;
}

#[tokio::test]
async fn test_workspace_fts_search() {
    let pool = get_pool();
    if try_connect(&pool).await.is_none() {
        return;
    }
    let user_id = "test_fts_search";
    cleanup_user(&pool, user_id).await;

    let workspace = Workspace::new(user_id, pool.clone());

    // Write some documents
    workspace
        .write(
            "docs/authentication.md",
            "# Authentication\n\nThe system uses JWT tokens for authentication.",
        )
        .await
        .expect("write failed");
    workspace
        .write(
            "docs/database.md",
            "# Database\n\nWe use PostgreSQL with pgvector for vector search.",
        )
        .await
        .expect("write failed");
    workspace
        .write(
            "docs/api.md",
            "# API\n\nThe REST API uses JSON for request and response bodies.",
        )
        .await
        .expect("write failed");

    // Search for JWT (FTS only since no embeddings)
    let results = workspace
        .search_with_config("JWT authentication", SearchConfig::default().fts_only())
        .await
        .expect("search failed");

    assert!(!results.is_empty(), "Should find results for JWT");
    assert!(
        results[0].content.contains("JWT"),
        "Top result should contain JWT"
    );

    // Search for PostgreSQL
    let results = workspace
        .search_with_config("PostgreSQL database", SearchConfig::default().fts_only())
        .await
        .expect("search failed");

    assert!(!results.is_empty(), "Should find results for PostgreSQL");
    assert!(
        results[0].content.contains("PostgreSQL"),
        "Top result should contain PostgreSQL"
    );

    cleanup_user(&pool, user_id).await;
}

#[tokio::test]
async fn test_workspace_hybrid_search_with_mock_embeddings() {
    let pool = get_pool();
    if try_connect(&pool).await.is_none() {
        return;
    }
    let user_id = "test_hybrid_search";
    cleanup_user(&pool, user_id).await;

    // Create workspace with mock embeddings (1536 dimensions to match OpenAI)
    let embeddings = Arc::new(MockEmbeddings::new(1536));
    let workspace = Workspace::new(user_id, pool.clone()).with_embeddings(embeddings);

    // Write documents
    workspace
        .write(
            "memory.md",
            "The user prefers dark mode and vim keybindings.",
        )
        .await
        .expect("write failed");
    workspace
        .write(
            "prefs.md",
            "Settings: theme=dark, editor=vim, font=monospace",
        )
        .await
        .expect("write failed");

    // Hybrid search
    let results = workspace
        .search("dark theme preference", 5)
        .await
        .expect("search failed");

    assert!(!results.is_empty(), "Should find results");
    // At least one result should be a hybrid match (found by both FTS and vector)
    // or we should have results from either method

    cleanup_user(&pool, user_id).await;
}
