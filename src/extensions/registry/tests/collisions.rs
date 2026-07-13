//! Extension registry collision tests (QA Plan P2 - 2.4) and builtin
//! relay-entry coverage.

use crate::extensions::registry::{ExtensionRegistry, builtin_entries_with_relay};
use crate::extensions::{AuthHint, ExtensionKind, ExtensionSource, RegistryEntry};

#[tokio::test]
async fn test_same_name_different_kind_both_discoverable() {
    // A WASM channel and WASM tool with the same name must coexist.
    let catalog_entries = vec![
        RegistryEntry {
            name: "telegram".to_string(),
            display_name: "Telegram Channel".to_string(),
            kind: ExtensionKind::WasmChannel,
            description: "Telegram messaging channel".to_string(),
            keywords: vec!["messaging".into()],
            source: ExtensionSource::WasmBuildable {
                source_dir: "channels-src/telegram".to_string(),
                build_dir: None,
                crate_name: None,
            },
            fallback_source: None,
            auth_hint: AuthHint::CapabilitiesAuth,
            version: None,
        },
        RegistryEntry {
            name: "telegram".to_string(),
            display_name: "Telegram Tool".to_string(),
            kind: ExtensionKind::WasmTool,
            description: "Telegram API tool".to_string(),
            keywords: vec!["messaging".into()],
            source: ExtensionSource::WasmBuildable {
                source_dir: "tools-src/telegram".to_string(),
                build_dir: None,
                crate_name: None,
            },
            fallback_source: None,
            auth_hint: AuthHint::CapabilitiesAuth,
            version: None,
        },
    ];

    let registry = ExtensionRegistry::new_with_catalog(catalog_entries);
    let all = registry.all_entries().await;

    // Both should exist since they have different kinds.
    let channel = all
        .iter()
        .find(|e| e.name == "telegram" && e.kind == ExtensionKind::WasmChannel);
    let tool = all
        .iter()
        .find(|e| e.name == "telegram" && e.kind == ExtensionKind::WasmTool);

    assert!(channel.is_some(), "Channel entry missing");
    assert!(tool.is_some(), "Tool entry missing");

    // Search should return both.
    let results = registry.search("telegram").await;
    let channel_hit = results
        .iter()
        .any(|r| r.entry.name == "telegram" && r.entry.kind == ExtensionKind::WasmChannel);
    let tool_hit = results
        .iter()
        .any(|r| r.entry.name == "telegram" && r.entry.kind == ExtensionKind::WasmTool);
    assert!(channel_hit, "Search should find channel");
    assert!(tool_hit, "Search should find tool");
}

#[tokio::test]
async fn test_get_returns_first_match_regardless_of_kind() {
    // `get()` returns the first entry with a matching name. If a channel
    // and tool share a name, callers that need a specific kind should
    // filter by kind.
    let catalog_entries = vec![
        RegistryEntry {
            name: "myext".to_string(),
            display_name: "MyExt Channel".to_string(),
            kind: ExtensionKind::WasmChannel,
            description: "Channel".to_string(),
            keywords: vec![],
            source: ExtensionSource::WasmBuildable {
                source_dir: "x".to_string(),
                build_dir: None,
                crate_name: None,
            },
            fallback_source: None,
            auth_hint: AuthHint::None,
            version: None,
        },
        RegistryEntry {
            name: "myext".to_string(),
            display_name: "MyExt Tool".to_string(),
            kind: ExtensionKind::WasmTool,
            description: "Tool".to_string(),
            keywords: vec![],
            source: ExtensionSource::WasmBuildable {
                source_dir: "y".to_string(),
                build_dir: None,
                crate_name: None,
            },
            fallback_source: None,
            auth_hint: AuthHint::None,
            version: None,
        },
    ];

    let registry = ExtensionRegistry::new_with_catalog(catalog_entries);

    // get() is name-only, returns first match.
    let entry = registry.get("myext").await;
    assert!(entry.is_some());
    // The first catalog entry added is the channel.
    assert_eq!(entry.unwrap().kind, ExtensionKind::WasmChannel);
}

#[test]
fn test_builtin_entries_with_relay_none_excludes_relay() {
    let entries = builtin_entries_with_relay(None);
    assert!(
        !entries
            .iter()
            .any(|e| e.kind == ExtensionKind::ChannelRelay),
        "No ChannelRelay entry when relay URL is None"
    );
}

#[test]
fn test_builtin_entries_with_relay_some_includes_relay() {
    let entries = builtin_entries_with_relay(Some("http://relay.example.com".to_string()));
    let relay = entries
        .iter()
        .find(|e| e.kind == ExtensionKind::ChannelRelay);
    assert!(relay.is_some(), "ChannelRelay entry should be present");
    if let ExtensionSource::ChannelRelay { relay_url } = &relay.unwrap().source {
        assert_eq!(relay_url, "http://relay.example.com");
    } else {
        panic!("Expected ChannelRelay source");
    }
}
