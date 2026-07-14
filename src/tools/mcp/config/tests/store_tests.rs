//! Tests for the servers-file model and disk load/save persistence.

use tempfile::tempdir;

use crate::tools::mcp::config::{
    McpServerConfig, McpServersFile, OAuthConfig, load_mcp_servers_from, save_mcp_servers_to,
};

#[test]
fn test_servers_file_operations() {
    let mut file = McpServersFile::default();

    // Add a server
    file.upsert(McpServerConfig::new("notion", "https://mcp.notion.com"));
    assert_eq!(file.servers.len(), 1);

    // Update the server
    let mut updated = McpServerConfig::new("notion", "https://mcp.notion.com/v2");
    updated.enabled = false;
    file.upsert(updated);
    assert_eq!(file.servers.len(), 1);
    assert!(!file.get("notion").unwrap().enabled);

    // Add another server
    file.upsert(McpServerConfig::new("github", "https://mcp.github.com"));
    assert_eq!(file.servers.len(), 2);

    // Remove a server
    assert!(file.remove("notion"));
    assert_eq!(file.servers.len(), 1);
    assert!(file.get("notion").is_none());

    // Remove non-existent server
    assert!(!file.remove("nonexistent"));
}

#[tokio::test]
async fn test_load_save_config() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("mcp-servers.json");

    // Save a configuration
    let mut config = McpServersFile::default();
    config.upsert(
        McpServerConfig::new("notion", "https://mcp.notion.com").with_oauth(
            OAuthConfig::new("client-123")
                .with_scopes(vec!["read".to_string(), "write".to_string()]),
        ),
    );

    save_mcp_servers_to(&config, &path).await.unwrap();

    // Load it back
    let loaded = load_mcp_servers_from(&path).await.unwrap();
    assert_eq!(loaded.servers.len(), 1);

    let server = loaded.get("notion").unwrap();
    assert_eq!(server.url, "https://mcp.notion.com");
    assert!(server.oauth.is_some());
    assert_eq!(server.oauth.as_ref().unwrap().client_id, "client-123");
}

#[tokio::test]
async fn test_load_nonexistent_returns_empty() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("nonexistent.json");

    let config = load_mcp_servers_from(&path).await.unwrap();
    assert!(config.servers.is_empty());
}
