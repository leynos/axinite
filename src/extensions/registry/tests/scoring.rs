//! Tests for search scoring, search ordering, and discovery caching.

use crate::extensions::registry::{ExtensionRegistry, score_entry};
use crate::extensions::{AuthHint, ExtensionKind, ExtensionSource, RegistryEntry};

#[test]
fn test_score_exact_name_match() {
    let entry = RegistryEntry {
        name: "notion".to_string(),
        display_name: "Notion".to_string(),
        kind: ExtensionKind::McpServer,
        description: "Workspace tool".to_string(),
        keywords: vec!["notes".into()],
        source: ExtensionSource::McpUrl {
            url: "https://example.com".to_string(),
        },
        fallback_source: None,
        auth_hint: AuthHint::Dcr,
        version: None,
    };

    let score = score_entry(&entry, &["notion".to_string()]);
    assert!(
        score >= 100,
        "Exact name match should score >= 100, got {}",
        score
    );
}

#[test]
fn test_score_partial_name_match() {
    let entry = RegistryEntry {
        name: "google-calendar".to_string(),
        display_name: "Google Calendar".to_string(),
        kind: ExtensionKind::McpServer,
        description: "Calendar management".to_string(),
        keywords: vec!["events".into()],
        source: ExtensionSource::McpUrl {
            url: "https://example.com".to_string(),
        },
        fallback_source: None,
        auth_hint: AuthHint::Dcr,
        version: None,
    };

    let score = score_entry(&entry, &["calendar".to_string()]);
    assert!(
        score > 0,
        "Partial name match should score > 0, got {}",
        score
    );
}

#[test]
fn test_score_keyword_match() {
    let entry = RegistryEntry {
        name: "notion".to_string(),
        display_name: "Notion".to_string(),
        kind: ExtensionKind::McpServer,
        description: "Workspace tool".to_string(),
        keywords: vec!["wiki".into(), "notes".into()],
        source: ExtensionSource::McpUrl {
            url: "https://example.com".to_string(),
        },
        fallback_source: None,
        auth_hint: AuthHint::Dcr,
        version: None,
    };

    let score = score_entry(&entry, &["wiki".to_string()]);
    assert!(
        score >= 40,
        "Exact keyword match should score >= 40, got {}",
        score
    );
}

#[test]
fn test_score_no_match() {
    let entry = RegistryEntry {
        name: "notion".to_string(),
        display_name: "Notion".to_string(),
        kind: ExtensionKind::McpServer,
        description: "Workspace tool".to_string(),
        keywords: vec!["notes".into()],
        source: ExtensionSource::McpUrl {
            url: "https://example.com".to_string(),
        },
        fallback_source: None,
        auth_hint: AuthHint::Dcr,
        version: None,
    };

    let score = score_entry(&entry, &["xyzfoobar".to_string()]);
    assert_eq!(score, 0, "No match should score 0");
}

#[tokio::test]
async fn test_search_returns_sorted() {
    let registry = ExtensionRegistry::new();
    let results = registry.search("notion").await;

    assert!(!results.is_empty(), "Should find notion in registry");
    assert_eq!(results[0].entry.name, "notion");
}

#[tokio::test]
async fn test_search_empty_query_returns_all() {
    let registry = ExtensionRegistry::new();
    let results = registry.search("").await;

    assert!(results.len() > 5, "Empty query should return all entries");
}

#[tokio::test]
async fn test_search_by_keyword() {
    let registry = ExtensionRegistry::new();
    let results = registry.search("issues tickets").await;

    assert!(
        !results.is_empty(),
        "Should find entries matching 'issues tickets'"
    );
    // Linear should be near the top since it has both keywords
    let linear_pos = results.iter().position(|r| r.entry.name == "linear");
    assert!(linear_pos.is_some(), "Linear should appear in results");
}

#[tokio::test]
async fn test_get_exact_name() {
    let registry = ExtensionRegistry::new();

    let entry = registry.get("notion").await;
    assert!(entry.is_some());
    assert_eq!(entry.unwrap().display_name, "Notion");

    let missing = registry.get("nonexistent").await;
    assert!(missing.is_none());
}

#[tokio::test]
async fn test_cache_discovered() {
    let registry = ExtensionRegistry::new();

    let discovered = RegistryEntry {
        name: "custom-mcp".to_string(),
        display_name: "Custom MCP".to_string(),
        kind: ExtensionKind::McpServer,
        description: "A custom MCP server".to_string(),
        keywords: vec![],
        source: ExtensionSource::McpUrl {
            url: "https://custom.example.com".to_string(),
        },
        fallback_source: None,
        auth_hint: AuthHint::Dcr,
        version: None,
    };

    registry.cache_discovered(vec![discovered]).await;

    let entry = registry.get("custom-mcp").await;
    assert!(entry.is_some());

    let results = registry.search("custom").await;
    assert!(!results.is_empty());
}

#[tokio::test]
async fn test_cache_deduplication() {
    let registry = ExtensionRegistry::new();

    let entry = RegistryEntry {
        name: "dup".to_string(),
        display_name: "Dup".to_string(),
        kind: ExtensionKind::McpServer,
        description: "Test".to_string(),
        keywords: vec![],
        source: ExtensionSource::McpUrl {
            url: "https://example.com".to_string(),
        },
        fallback_source: None,
        auth_hint: AuthHint::None,
        version: None,
    };

    registry.cache_discovered(vec![entry.clone()]).await;
    registry.cache_discovered(vec![entry]).await;

    let results = registry.search("dup").await;
    assert_eq!(results.len(), 1, "Should not duplicate cached entries");
}
