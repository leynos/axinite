//! Unit tests for parsing registry manifest JSON.

use super::*;

#[test]
fn test_parse_tool_manifest() {
    let json = r#"{
        "name": "slack",
        "display_name": "Slack",
        "kind": "tool",
        "version": "0.1.0",
        "description": "Post messages via Slack API",
        "keywords": ["messaging"],
        "source": {
            "dir": "tools-src/slack",
            "capabilities": "slack-tool.capabilities.json",
            "crate_name": "slack-tool"
        },
        "artifacts": {
            "wasm32-wasip2": { "url": null, "sha256": null }
        },
        "auth_summary": {
            "method": "oauth",
            "provider": "Slack",
            "secrets": ["slack_bot_token"],
            "shared_auth": null,
            "setup_url": "https://api.slack.com/apps"
        },
        "tags": ["default", "messaging"]
    }"#;

    let manifest: ExtensionManifest = serde_json::from_str(json).expect("parse manifest");
    assert_eq!(manifest.name, "slack");
    assert_eq!(manifest.kind, ManifestKind::Tool);
    assert_eq!(manifest.version, "0.1.0");
    assert!(manifest.tags.contains(&"default".to_string()));

    let entry = manifest.to_registry_entry();
    assert_eq!(entry.kind, ExtensionKind::WasmTool);
}

#[test]
fn test_parse_channel_manifest() {
    let json = r#"{
        "name": "telegram",
        "display_name": "Telegram",
        "kind": "channel",
        "version": "0.1.0",
        "description": "Telegram Bot API channel",
        "source": {
            "dir": "channels-src/telegram",
            "capabilities": "telegram.capabilities.json",
            "crate_name": "telegram-channel"
        },
        "tags": ["messaging"]
    }"#;

    let manifest: ExtensionManifest = serde_json::from_str(json).expect("parse manifest");
    assert_eq!(manifest.kind, ManifestKind::Channel);
    assert!(manifest.auth_summary.is_none());
    assert!(manifest.artifacts.is_empty());

    let entry = manifest.to_registry_entry();
    assert_eq!(entry.kind, ExtensionKind::WasmChannel);
}

#[test]
fn test_parse_bundles() {
    let json = r#"{
        "bundles": {
            "google": {
                "display_name": "Google Suite",
                "description": "All Google tools",
                "extensions": ["tools/gmail", "tools/google-calendar"],
                "shared_auth": "google_oauth_token"
            },
            "default": {
                "display_name": "Recommended Set",
                "extensions": ["tools/github", "tools/slack"]
            }
        }
    }"#;

    let bundles: BundlesFile = serde_json::from_str(json).expect("parse bundles");
    assert_eq!(bundles.bundles.len(), 2);
    assert_eq!(
        bundles.bundles["google"].shared_auth.as_deref(),
        Some("google_oauth_token")
    );
    assert!(bundles.bundles["default"].shared_auth.is_none());
}

#[test]
fn test_manifest_kind_display() {
    assert_eq!(ManifestKind::Tool.to_string(), "tool");
    assert_eq!(ManifestKind::Channel.to_string(), "channel");
}

/// When a manifest has a download URL in artifacts, to_registry_entry()
/// should set WasmDownload as primary source and WasmBuildable as fallback.
#[test]
fn test_manifest_with_download_url_has_buildable_fallback() {
    let json = r#"{
        "name": "gmail",
        "display_name": "Gmail",
        "kind": "tool",
        "version": "0.1.0",
        "description": "Gmail tool",
        "keywords": ["email"],
        "source": {
            "dir": "tools-src/gmail",
            "capabilities": "gmail-tool.capabilities.json",
            "crate_name": "gmail-tool"
        },
        "artifacts": {
            "wasm32-wasip2": {
                "url": "https://github.com/nearai/ironclaw/releases/latest/download/gmail-wasm32-wasip2.tar.gz",
                "sha256": null
            }
        },
        "tags": ["default"]
    }"#;

    let manifest: ExtensionManifest = serde_json::from_str(json).expect("parse manifest");
    let entry = manifest.to_registry_entry();

    // Primary source should be WasmDownload
    assert!(
        matches!(&entry.source, ExtensionSource::WasmDownload { .. }),
        "Primary source should be WasmDownload, got {:?}",
        entry.source
    );

    // Fallback should be WasmBuildable with the source dir info
    let fallback = entry
        .fallback_source
        .as_ref()
        .expect("Should have fallback_source when download URL is set");
    match fallback.as_ref() {
        ExtensionSource::WasmBuildable {
            build_dir,
            crate_name,
            ..
        } => {
            assert_eq!(build_dir.as_deref(), Some("tools-src/gmail"));
            assert_eq!(crate_name.as_deref(), Some("gmail-tool"));
        }
        other => panic!("Fallback should be WasmBuildable, got {:?}", other),
    }
}

/// When a manifest has null URL in artifacts, the primary source should be
/// WasmBuildable with no fallback.
#[test]
fn test_manifest_with_null_url_no_fallback() {
    let json = r#"{
        "name": "slack",
        "display_name": "Slack",
        "kind": "tool",
        "version": "0.1.0",
        "description": "Slack tool",
        "keywords": [],
        "source": {
            "dir": "tools-src/slack",
            "capabilities": "slack-tool.capabilities.json",
            "crate_name": "slack-tool"
        },
        "artifacts": {
            "wasm32-wasip2": { "url": null, "sha256": null }
        },
        "tags": []
    }"#;

    let manifest: ExtensionManifest = serde_json::from_str(json).expect("parse manifest");
    let entry = manifest.to_registry_entry();

    assert!(
        matches!(&entry.source, ExtensionSource::WasmBuildable { .. }),
        "Should use WasmBuildable when URL is null"
    );
    assert!(
        entry.fallback_source.is_none(),
        "Should have no fallback when already using WasmBuildable"
    );
}

/// When a manifest has no artifacts section, should use WasmBuildable with no fallback.
#[test]
fn test_manifest_no_artifacts_no_fallback() {
    let json = r#"{
        "name": "custom",
        "display_name": "Custom",
        "kind": "tool",
        "version": "0.1.0",
        "description": "Custom tool",
        "keywords": [],
        "source": {
            "dir": "tools-src/custom",
            "capabilities": "custom.capabilities.json",
            "crate_name": "custom-tool"
        },
        "tags": []
    }"#;

    let manifest: ExtensionManifest = serde_json::from_str(json).expect("parse manifest");
    let entry = manifest.to_registry_entry();

    assert!(
        matches!(&entry.source, ExtensionSource::WasmBuildable { .. }),
        "Should use WasmBuildable when no artifacts"
    );
    assert!(
        entry.fallback_source.is_none(),
        "Should have no fallback when already using WasmBuildable"
    );
}
