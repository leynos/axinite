//! Unit tests for extension discovery helpers and MCP URL validation.

use crate::extensions::ExtensionSource;
use crate::extensions::discovery::{
    OnlineDiscovery, extract_source, titlecase, validate_mcp_url_with_client,
};

#[test]
fn test_titlecase() {
    assert_eq!(titlecase("google calendar"), "Google Calendar");
    assert_eq!(titlecase("notion"), "Notion");
    assert_eq!(titlecase(""), "");
}

#[test]
fn test_extract_source() {
    let mcp = ExtensionSource::McpUrl {
        url: "https://mcp.notion.com".to_string(),
    };
    assert_eq!(extract_source(&mcp), "https://mcp.notion.com");

    let discovered = ExtensionSource::Discovered {
        url: "https://example.com".to_string(),
    };
    assert_eq!(extract_source(&discovered), "https://example.com");
}

#[tokio::test]
async fn test_validate_invalid_url() {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(3))
        .build()
        .unwrap();

    // Invalid URL should fail
    assert!(!validate_mcp_url_with_client(&client, "not-a-url").await);
}

#[test]
fn test_discovery_new() {
    // Just make sure it constructs without panicking
    let _discovery = OnlineDiscovery::new();
}

#[test]
fn test_titlecase_single_char() {
    assert_eq!(titlecase("a"), "A");
    assert_eq!(titlecase("Z"), "Z");
}

#[test]
fn test_titlecase_mixed_case() {
    assert_eq!(titlecase("hELLO wORLD"), "HELLO WORLD");
    // Only first char is uppercased, rest is left as-is
    assert_eq!(titlecase("alREADY weird"), "AlREADY Weird");
}

#[test]
fn test_titlecase_multiple_spaces() {
    // split_whitespace collapses multiple spaces
    assert_eq!(titlecase("hello   world"), "Hello World");
    assert_eq!(titlecase("  leading trailing  "), "Leading Trailing");
}

#[test]
fn test_titlecase_punctuation() {
    assert_eq!(titlecase("hello-world"), "Hello-world");
    assert_eq!(titlecase("it's fine"), "It's Fine");
    assert_eq!(titlecase("one. two"), "One. Two");
}

#[test]
fn test_extract_source_wasm_download() {
    let src = ExtensionSource::WasmDownload {
        wasm_url: "https://example.com/tool.wasm".to_string(),
        capabilities_url: Some("https://example.com/caps.json".to_string()),
    };
    assert_eq!(extract_source(&src), "https://example.com/tool.wasm");

    let src_no_caps = ExtensionSource::WasmDownload {
        wasm_url: "https://other.com/bin.wasm".to_string(),
        capabilities_url: None,
    };
    assert_eq!(extract_source(&src_no_caps), "https://other.com/bin.wasm");
}

#[test]
fn test_extract_source_wasm_buildable() {
    let src = ExtensionSource::WasmBuildable {
        source_dir: "/home/user/my-tool".to_string(),
        build_dir: Some("/home/user/my-tool/target".to_string()),
        crate_name: Some("my_tool".to_string()),
    };
    assert_eq!(extract_source(&src), "/home/user/my-tool");

    let src_minimal = ExtensionSource::WasmBuildable {
        source_dir: "./src".to_string(),
        build_dir: None,
        crate_name: None,
    };
    assert_eq!(extract_source(&src_minimal), "./src");
}

#[test]
fn test_online_discovery_default() {
    let d = OnlineDiscovery::default();
    // Verify it constructed (no panic) and the client is usable
    let _ = d.http_client;
}

#[test]
fn test_github_search_response_empty_items() {
    let json = r#"{"total_count": 0, "items": []}"#;
    let resp: super::GitHubSearchResponse = serde_json::from_str(json).unwrap();
    assert!(resp.items.is_empty());
}

#[test]
fn test_github_search_response_missing_items_field() {
    // items has #[serde(default)], so missing field should give empty vec
    let json = r#"{"total_count": 0}"#;
    let resp: super::GitHubSearchResponse = serde_json::from_str(json).unwrap();
    assert!(resp.items.is_empty());
}

#[test]
fn test_github_search_response_multiple_items() {
    let json = r#"{
        "items": [
            {
                "name": "mcp-server-a",
                "full_name": "org/mcp-server-a",
                "html_url": "https://github.com/org/mcp-server-a",
                "description": "First server",
                "topics": ["mcp"]
            },
            {
                "name": "mcp-server-b",
                "full_name": "org/mcp-server-b",
                "html_url": "https://github.com/org/mcp-server-b",
                "description": null,
                "topics": ["mcp", "tools"]
            }
        ]
    }"#;
    let resp: super::GitHubSearchResponse = serde_json::from_str(json).unwrap();
    assert_eq!(resp.items.len(), 2);
    assert_eq!(resp.items[0].name, "mcp-server-a");
    assert_eq!(resp.items[1].name, "mcp-server-b");
    assert_eq!(resp.items[0].description, Some("First server".to_string()));
    assert!(resp.items[1].description.is_none());
}

#[test]
fn test_github_repo_all_fields() {
    let json = r#"{
        "name": "cool-mcp",
        "full_name": "user/cool-mcp",
        "html_url": "https://github.com/user/cool-mcp",
        "description": "A cool MCP server",
        "homepage": "https://cool-mcp.dev",
        "topics": ["mcp-server", "model-context-protocol", "rust"]
    }"#;
    let repo: super::GitHubRepo = serde_json::from_str(json).unwrap();
    assert_eq!(repo.name, "cool-mcp");
    assert_eq!(repo.full_name, "user/cool-mcp");
    assert_eq!(repo.html_url, "https://github.com/user/cool-mcp");
    assert_eq!(repo.description.as_deref(), Some("A cool MCP server"));
    assert_eq!(repo.homepage.as_deref(), Some("https://cool-mcp.dev"));
    assert_eq!(repo.topics.len(), 3);
}

#[test]
fn test_github_repo_missing_optional_fields() {
    let json = r#"{
        "name": "bare-repo",
        "full_name": "user/bare-repo",
        "html_url": "https://github.com/user/bare-repo"
    }"#;
    let repo: super::GitHubRepo = serde_json::from_str(json).unwrap();
    assert_eq!(repo.name, "bare-repo");
    assert!(repo.description.is_none());
    assert!(repo.homepage.is_none());
    assert!(repo.topics.is_empty());
}

#[tokio::test]
async fn test_with_timeout_completes() {
    use crate::extensions::discovery::with_timeout;

    let result = with_timeout(async { 42 }, std::time::Duration::from_secs(1)).await;
    assert_eq!(result, Some(42));
}

#[tokio::test]
async fn test_with_timeout_expires() {
    use crate::extensions::discovery::with_timeout;

    let result = with_timeout(
        tokio::time::sleep(std::time::Duration::from_secs(5)),
        std::time::Duration::from_millis(10),
    )
    .await;
    assert!(result.is_none());
}

#[tokio::test]
async fn test_discover_empty_query() {
    let discovery = OnlineDiscovery::new();
    let results = discovery.discover("").await;
    assert!(results.is_empty());
}

#[tokio::test]
async fn test_discover_whitespace_only_query() {
    let discovery = OnlineDiscovery::new();
    let results = discovery.discover("   \t\n  ").await;
    assert!(results.is_empty());
}
