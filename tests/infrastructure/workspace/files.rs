//! Workspace file read, write, append, delete, and listing tests.
//!
//! Requires a running PostgreSQL with pgvector extension.

use super::{cleanup_user, get_pool, try_connect};
use axinite::workspace::{Workspace, paths};

#[tokio::test]
async fn test_workspace_write_and_read() {
    let pool = get_pool();
    if try_connect(&pool).await.is_none() {
        return;
    }
    let user_id = "test_write_read";
    cleanup_user(&pool, user_id).await;

    let workspace = Workspace::new(user_id, pool.clone());

    // Write a file
    let doc = workspace
        .write("README.md", "# Hello World\n\nThis is a test.")
        .await
        .expect("Failed to write");

    assert_eq!(doc.path, "README.md");
    assert!(doc.content.contains("Hello World"));

    // Read it back
    let doc2 = workspace.read("README.md").await.expect("Failed to read");
    assert_eq!(doc2.content, "# Hello World\n\nThis is a test.");

    // Cleanup
    cleanup_user(&pool, user_id).await;
}

#[tokio::test]
async fn test_workspace_append() {
    let pool = get_pool();
    if try_connect(&pool).await.is_none() {
        return;
    }
    let user_id = "test_append";
    cleanup_user(&pool, user_id).await;

    let workspace = Workspace::new(user_id, pool.clone());

    // Write initial content
    workspace
        .write("notes.md", "Line 1")
        .await
        .expect("Failed to write");

    // Append more
    workspace
        .append("notes.md", "Line 2")
        .await
        .expect("Failed to append");

    // Read and verify
    let doc = workspace.read("notes.md").await.expect("Failed to read");
    assert_eq!(doc.content, "Line 1\nLine 2");

    cleanup_user(&pool, user_id).await;
}

#[tokio::test]
async fn test_workspace_nested_paths() {
    let pool = get_pool();
    if try_connect(&pool).await.is_none() {
        return;
    }
    let user_id = "test_nested";
    cleanup_user(&pool, user_id).await;

    let workspace = Workspace::new(user_id, pool.clone());

    // Write nested files
    workspace
        .write("projects/alpha/README.md", "# Alpha")
        .await
        .expect("Failed to write alpha");
    workspace
        .write("projects/alpha/notes.md", "Notes here")
        .await
        .expect("Failed to write notes");
    workspace
        .write("projects/beta/README.md", "# Beta")
        .await
        .expect("Failed to write beta");

    // List root
    let root = workspace.list("").await.expect("Failed to list root");
    assert_eq!(root.len(), 1); // just "projects/"
    assert!(root[0].is_directory);
    assert_eq!(root[0].name(), "projects");

    // List projects
    let projects = workspace
        .list("projects")
        .await
        .expect("Failed to list projects");
    assert_eq!(projects.len(), 2); // alpha/, beta/

    // List alpha
    let alpha = workspace
        .list("projects/alpha")
        .await
        .expect("Failed to list alpha");
    assert_eq!(alpha.len(), 2); // README.md, notes.md

    cleanup_user(&pool, user_id).await;
}

#[tokio::test]
async fn test_workspace_delete() {
    let pool = get_pool();
    if try_connect(&pool).await.is_none() {
        return;
    }
    let user_id = "test_delete";
    cleanup_user(&pool, user_id).await;

    let workspace = Workspace::new(user_id, pool.clone());

    // Write and verify exists
    workspace
        .write("temp.md", "temporary")
        .await
        .expect("Failed to write");
    assert!(workspace.exists("temp.md").await.expect("exists failed"));

    // Delete
    workspace.delete("temp.md").await.expect("Failed to delete");

    // Verify gone
    assert!(!workspace.exists("temp.md").await.expect("exists failed"));

    cleanup_user(&pool, user_id).await;
}

#[tokio::test]
async fn test_workspace_list_all() {
    let pool = get_pool();
    if try_connect(&pool).await.is_none() {
        return;
    }
    let user_id = "test_list_all";
    cleanup_user(&pool, user_id).await;

    let workspace = Workspace::new(user_id, pool.clone());

    // Write files at various depths
    workspace.write("README.md", "root").await.unwrap();
    workspace.write("docs/intro.md", "intro").await.unwrap();
    workspace.write("docs/api/rest.md", "rest").await.unwrap();
    workspace.write("src/main.md", "main").await.unwrap();

    // List all
    let all = workspace.list_all().await.expect("list_all failed");
    assert_eq!(all.len(), 4);
    assert!(all.contains(&"README.md".to_string()));
    assert!(all.contains(&"docs/intro.md".to_string()));
    assert!(all.contains(&"docs/api/rest.md".to_string()));
    assert!(all.contains(&"src/main.md".to_string()));

    cleanup_user(&pool, user_id).await;
}

#[tokio::test]
async fn test_workspace_system_prompt() {
    let pool = get_pool();
    if try_connect(&pool).await.is_none() {
        return;
    }
    let user_id = "test_system_prompt";
    cleanup_user(&pool, user_id).await;

    let workspace = Workspace::new(user_id, pool.clone());

    // Write identity files
    workspace
        .write(paths::AGENTS, "You are a helpful assistant.")
        .await
        .unwrap();
    workspace
        .write(paths::SOUL, "Be kind and thorough.")
        .await
        .unwrap();
    workspace.write(paths::USER, "Name: Alice").await.unwrap();

    // Get system prompt
    let prompt = workspace
        .system_prompt()
        .await
        .expect("system_prompt failed");

    assert!(
        prompt.contains("helpful assistant"),
        "Should include AGENTS.md"
    );
    assert!(
        prompt.contains("kind and thorough"),
        "Should include SOUL.md"
    );
    assert!(prompt.contains("Alice"), "Should include USER.md");

    cleanup_user(&pool, user_id).await;
}
