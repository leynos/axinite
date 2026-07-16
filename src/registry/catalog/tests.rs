//! Unit tests for loading and querying the extension registry catalogue.

use super::*;
use ambient_fs as fs;

fn create_test_registry(dir: &Path) {
    let tools_dir = dir.join("tools");
    let channels_dir = dir.join("channels");
    fs::create_dir_all(&tools_dir).unwrap();
    fs::create_dir_all(&channels_dir).unwrap();

    fs::write(
        tools_dir.join("slack.json"),
        r#"{
            "name": "slack",
            "display_name": "Slack",
            "kind": "tool",
            "version": "0.1.0",
            "description": "Post messages via Slack API",
            "keywords": ["messaging", "chat"],
            "source": {
                "dir": "tools-src/slack",
                "capabilities": "slack-tool.capabilities.json",
                "crate_name": "slack-tool"
            },
            "auth_summary": {
                "method": "oauth",
                "provider": "Slack",
                "secrets": ["slack_bot_token"]
            },
            "tags": ["default", "messaging"]
        }"#,
    )
    .unwrap();

    fs::write(
        tools_dir.join("github.json"),
        r#"{
            "name": "github",
            "display_name": "GitHub",
            "kind": "tool",
            "version": "0.1.0",
            "description": "GitHub integration for issues and PRs",
            "keywords": ["code", "git"],
            "source": {
                "dir": "tools-src/github",
                "capabilities": "github-tool.capabilities.json",
                "crate_name": "github-tool"
            },
            "tags": ["default", "development"]
        }"#,
    )
    .unwrap();

    fs::write(
        channels_dir.join("telegram.json"),
        r#"{
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
        }"#,
    )
    .unwrap();

    fs::write(
        dir.join("_bundles.json"),
        r#"{
            "bundles": {
                "default": {
                    "display_name": "Recommended",
                    "extensions": ["tools/slack", "tools/github", "channels/telegram"]
                },
                "messaging": {
                    "display_name": "Messaging",
                    "extensions": ["tools/slack", "channels/telegram"],
                    "shared_auth": null
                }
            }
        }"#,
    )
    .unwrap();
}

#[test]
fn test_load_catalog() {
    let tmp = tempfile::tempdir().unwrap();
    create_test_registry(tmp.path());

    let catalog = RegistryCatalog::load(tmp.path()).unwrap();
    assert_eq!(catalog.all().len(), 3);
}

#[test]
fn test_list_by_kind() {
    let tmp = tempfile::tempdir().unwrap();
    create_test_registry(tmp.path());

    let catalog = RegistryCatalog::load(tmp.path()).unwrap();
    let tools = catalog.list(Some(ManifestKind::Tool), None);
    assert_eq!(tools.len(), 2);

    let channels = catalog.list(Some(ManifestKind::Channel), None);
    assert_eq!(channels.len(), 1);
}

#[test]
fn test_list_by_tag() {
    let tmp = tempfile::tempdir().unwrap();
    create_test_registry(tmp.path());

    let catalog = RegistryCatalog::load(tmp.path()).unwrap();
    let defaults = catalog.list(None, Some("default"));
    assert_eq!(defaults.len(), 2);

    let messaging = catalog.list(None, Some("messaging"));
    assert_eq!(messaging.len(), 2); // slack (tool) and telegram (channel) both have "messaging" tag
}

#[test]
fn test_get_by_name() {
    let tmp = tempfile::tempdir().unwrap();
    create_test_registry(tmp.path());

    let catalog = RegistryCatalog::load(tmp.path()).unwrap();

    // Full key
    assert!(catalog.get("tools/slack").is_some());

    // Bare name
    assert!(catalog.get("slack").is_some());
    assert!(catalog.get("telegram").is_some());

    // Missing
    assert!(catalog.get("nonexistent").is_none());
}

#[test]
fn test_search() {
    let tmp = tempfile::tempdir().unwrap();
    create_test_registry(tmp.path());

    let catalog = RegistryCatalog::load(tmp.path()).unwrap();

    let results = catalog.search("slack");
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].name, "slack");

    let results = catalog.search("messaging");
    assert!(!results.is_empty());

    let results = catalog.search("nonexistent query");
    assert!(results.is_empty());
}

#[test]
fn test_resolve_bundle() {
    let tmp = tempfile::tempdir().unwrap();
    create_test_registry(tmp.path());

    let catalog = RegistryCatalog::load(tmp.path()).unwrap();

    let (manifests, missing) = catalog.resolve_bundle("default").unwrap();
    assert_eq!(manifests.len(), 3);
    assert!(missing.is_empty());

    assert!(catalog.resolve_bundle("nonexistent").is_err());
}

#[test]
fn test_resolve_single_or_bundle() {
    let tmp = tempfile::tempdir().unwrap();
    create_test_registry(tmp.path());

    let catalog = RegistryCatalog::load(tmp.path()).unwrap();

    // Single extension
    let (manifests, bundle) = catalog.resolve("slack").unwrap();
    assert_eq!(manifests.len(), 1);
    assert!(bundle.is_none());

    // Bundle
    let (manifests, bundle) = catalog.resolve("default").unwrap();
    assert_eq!(manifests.len(), 3);
    assert!(bundle.is_some());
}

#[test]
fn test_bundle_names() {
    let tmp = tempfile::tempdir().unwrap();
    create_test_registry(tmp.path());

    let catalog = RegistryCatalog::load(tmp.path()).unwrap();
    let names = catalog.bundle_names();
    assert_eq!(names, vec!["default", "messaging"]);
}

#[test]
fn test_directory_not_found() {
    let result = RegistryCatalog::load(Path::new("/nonexistent/path"));
    assert!(result.is_err());
}

#[test]
fn test_load_or_embedded_succeeds() {
    // Should always succeed: either finds registry/ on disk or falls back to embedded
    let catalog = RegistryCatalog::load_or_embedded().unwrap();
    // At minimum, the embedded catalog from the repo should have entries
    assert!(!catalog.all().is_empty() || !catalog.bundle_names().is_empty());
}

#[test]
fn test_bundle_entries_resolve_against_real_registry() {
    // Load the actual registry/ directory (catches stale bundle refs after renames)
    let catalog = RegistryCatalog::load_or_embedded().unwrap();

    for bundle_name in catalog.bundle_names() {
        let (manifests, missing) = catalog.resolve_bundle(bundle_name).unwrap();
        assert!(
            missing.is_empty(),
            "Bundle '{}' has unresolved entries: {:?}. \
             Check that _bundles.json entries match manifest name fields.",
            bundle_name,
            missing
        );
        assert!(
            !manifests.is_empty(),
            "Bundle '{}' resolved to zero manifests",
            bundle_name
        );
    }
}
