//! Tests for catalog merging and kind-aware entry lookup.

use crate::extensions::registry::ExtensionRegistry;
use crate::extensions::{AuthHint, ExtensionKind, ExtensionSource, RegistryEntry};

#[tokio::test]
async fn test_new_with_catalog() {
    let catalog_entries = vec![
        RegistryEntry {
            name: "telegram".to_string(),
            display_name: "Telegram".to_string(),
            kind: ExtensionKind::WasmChannel,
            description: "Telegram Bot API channel".to_string(),
            keywords: vec!["messaging".into(), "bot".into()],
            source: ExtensionSource::WasmBuildable {
                source_dir: "channels-src/telegram".to_string(),
                build_dir: Some("channels-src/telegram".to_string()),
                crate_name: Some("telegram-channel".to_string()),
            },
            fallback_source: None,
            auth_hint: AuthHint::CapabilitiesAuth,
            version: None,
        },
        // This shares a name with the builtin slack-mcp but has a different kind, so both should appear
        RegistryEntry {
            name: "slack-mcp".to_string(),
            display_name: "Slack MCP WASM".to_string(),
            kind: ExtensionKind::WasmTool,
            description: "Slack WASM tool".to_string(),
            keywords: vec!["messaging".into()],
            source: ExtensionSource::WasmBuildable {
                source_dir: "tools-src/slack".to_string(),
                build_dir: Some("tools-src/slack".to_string()),
                crate_name: Some("slack-tool".to_string()),
            },
            fallback_source: None,
            auth_hint: AuthHint::CapabilitiesAuth,
            version: None,
        },
    ];

    let registry = ExtensionRegistry::new_with_catalog(catalog_entries);

    // Should find the new telegram entry
    let results = registry.search("telegram").await;
    assert!(!results.is_empty(), "Should find telegram from catalog");
    assert_eq!(results[0].entry.name, "telegram");

    // Should have both builtin MCP slack-mcp and catalog WASM slack-mcp
    let results = registry.search("slack").await;
    let slack_mcp = results
        .iter()
        .any(|r| r.entry.name == "slack-mcp" && r.entry.kind == ExtensionKind::McpServer);
    let slack_wasm = results
        .iter()
        .any(|r| r.entry.name == "slack-mcp" && r.entry.kind == ExtensionKind::WasmTool);
    assert!(slack_mcp, "Should have builtin MCP slack-mcp");
    assert!(slack_wasm, "Should have catalog WASM slack-mcp");
}

#[tokio::test]
async fn test_new_with_catalog_dedup_same_kind() {
    // A catalog entry with same name AND kind as a builtin should be skipped
    let catalog_entries = vec![RegistryEntry {
        name: "slack-mcp".to_string(),
        display_name: "Slack MCP Override".to_string(),
        kind: ExtensionKind::McpServer, // same kind as builtin slack-mcp
        description: "Should be skipped".to_string(),
        keywords: vec![],
        source: ExtensionSource::McpUrl {
            url: "https://other.slack.com".to_string(),
        },
        fallback_source: None,
        auth_hint: AuthHint::Dcr,
        version: None,
    }];

    let registry = ExtensionRegistry::new_with_catalog(catalog_entries);

    let entry = registry.get("slack-mcp").await;
    assert!(entry.is_some());
    // Should still be the builtin, not the override
    assert_eq!(entry.unwrap().display_name, "Slack MCP");
}

#[tokio::test]
async fn test_get_with_kind_resolves_collision() {
    // Two entries with the same name but different kinds (the telegram collision scenario)
    let catalog_entries = vec![
        RegistryEntry {
            name: "telegram".to_string(),
            display_name: "Telegram Tool".to_string(),
            kind: ExtensionKind::WasmTool,
            description: "Telegram MTProto tool".to_string(),
            keywords: vec!["messaging".into()],
            source: ExtensionSource::WasmBuildable {
                source_dir: "tools-src/telegram".to_string(),
                build_dir: Some("tools-src/telegram".to_string()),
                crate_name: Some("telegram-tool".to_string()),
            },
            fallback_source: None,
            auth_hint: AuthHint::CapabilitiesAuth,
            version: None,
        },
        RegistryEntry {
            name: "telegram".to_string(),
            display_name: "Telegram Channel".to_string(),
            kind: ExtensionKind::WasmChannel,
            description: "Telegram Bot API channel".to_string(),
            keywords: vec!["messaging".into(), "bot".into()],
            source: ExtensionSource::WasmBuildable {
                source_dir: "channels-src/telegram".to_string(),
                build_dir: Some("channels-src/telegram".to_string()),
                crate_name: Some("telegram-channel".to_string()),
            },
            fallback_source: None,
            auth_hint: AuthHint::CapabilitiesAuth,
            version: None,
        },
    ];

    let registry = ExtensionRegistry::new_with_catalog(catalog_entries);

    // Without kind hint, get() returns the first match (WasmTool)
    let entry = registry.get("telegram").await;
    assert!(entry.is_some());
    assert_eq!(entry.unwrap().kind, ExtensionKind::WasmTool);

    // With kind hint for WasmChannel, get_with_kind() returns the channel entry
    let entry = registry
        .get_with_kind("telegram", Some(ExtensionKind::WasmChannel))
        .await;
    assert!(entry.is_some());
    let entry = entry.unwrap();
    assert_eq!(entry.kind, ExtensionKind::WasmChannel);
    assert_eq!(entry.display_name, "Telegram Channel");

    // With kind hint for WasmTool, get_with_kind() returns the tool entry
    let entry = registry
        .get_with_kind("telegram", Some(ExtensionKind::WasmTool))
        .await;
    assert!(entry.is_some());
    let entry = entry.unwrap();
    assert_eq!(entry.kind, ExtensionKind::WasmTool);
    assert_eq!(entry.display_name, "Telegram Tool");

    // Without kind hint (None), get_with_kind() falls back to first match
    let entry = registry.get_with_kind("telegram", None).await;
    assert!(entry.is_some());
    assert_eq!(entry.unwrap().kind, ExtensionKind::WasmTool);

    // Kind mismatch: no McpServer named "telegram" exists — must return None,
    // not silently fall back to the WasmTool entry.
    let entry = registry
        .get_with_kind("telegram", Some(ExtensionKind::McpServer))
        .await;
    assert!(
        entry.is_none(),
        "Should return None when kind doesn't match, not fall back to wrong kind"
    );
}

#[tokio::test]
async fn test_get_with_kind_discovery_cache() {
    let registry = ExtensionRegistry::new();

    // Add two entries with the same name but different kinds to the discovery cache
    let tool_entry = RegistryEntry {
        name: "cached-ext".to_string(),
        display_name: "Cached Tool".to_string(),
        kind: ExtensionKind::WasmTool,
        description: "A cached tool".to_string(),
        keywords: vec![],
        source: ExtensionSource::WasmBuildable {
            source_dir: "tools-src/cached".to_string(),
            build_dir: None,
            crate_name: None,
        },
        fallback_source: None,
        auth_hint: AuthHint::None,
        version: None,
    };
    let channel_entry = RegistryEntry {
        name: "cached-ext".to_string(),
        display_name: "Cached Channel".to_string(),
        kind: ExtensionKind::WasmChannel,
        description: "A cached channel".to_string(),
        keywords: vec![],
        source: ExtensionSource::WasmBuildable {
            source_dir: "channels-src/cached".to_string(),
            build_dir: None,
            crate_name: None,
        },
        fallback_source: None,
        auth_hint: AuthHint::None,
        version: None,
    };

    registry
        .cache_discovered(vec![tool_entry, channel_entry])
        .await;

    // Kind-aware lookup should find the channel in the cache
    let entry = registry
        .get_with_kind("cached-ext", Some(ExtensionKind::WasmChannel))
        .await;
    assert!(entry.is_some());
    assert_eq!(entry.unwrap().display_name, "Cached Channel");

    // Kind-aware lookup should find the tool in the cache
    let entry = registry
        .get_with_kind("cached-ext", Some(ExtensionKind::WasmTool))
        .await;
    assert!(entry.is_some());
    assert_eq!(entry.unwrap().display_name, "Cached Tool");
}

// Channel tests (telegram, slack, discord, whatsapp) require the embedded catalog
// to be loaded via new_with_catalog(). See test_new_with_catalog for catalog coverage.
