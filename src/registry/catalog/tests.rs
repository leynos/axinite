//! Unit tests for loading and querying the extension registry catalogue.

use super::*;
use ambient_fs as fs;
use anyhow::Context as _;

fn create_test_registry(dir: &Path) -> anyhow::Result<()> {
    let tools_dir = dir.join("tools");
    let channels_dir = dir.join("channels");
    fs::create_dir_all(&tools_dir)
        .with_context(|| format!("create tools dir {}", tools_dir.display()))?;
    fs::create_dir_all(&channels_dir)
        .with_context(|| format!("create channels dir {}", channels_dir.display()))?;

    let slack_path = tools_dir.join("slack.json");
    fs::write(
        &slack_path,
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
    .with_context(|| format!("write {}", slack_path.display()))?;

    let github_path = tools_dir.join("github.json");
    fs::write(
        &github_path,
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
    .with_context(|| format!("write {}", github_path.display()))?;

    let telegram_path = channels_dir.join("telegram.json");
    fs::write(
        &telegram_path,
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
    .with_context(|| format!("write {}", telegram_path.display()))?;

    let bundles_path = dir.join("_bundles.json");
    fs::write(
        &bundles_path,
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
    .with_context(|| format!("write {}", bundles_path.display()))?;
    Ok(())
}

#[test]
fn test_load_catalog() -> anyhow::Result<()> {
    let tmp = tempfile::tempdir()?;
    create_test_registry(tmp.path())?;

    let catalog = RegistryCatalog::load(tmp.path())?;
    assert_eq!(catalog.all().len(), 3);
    Ok(())
}

#[test]
fn test_list_by_kind() -> anyhow::Result<()> {
    let tmp = tempfile::tempdir()?;
    create_test_registry(tmp.path())?;

    let catalog = RegistryCatalog::load(tmp.path())?;
    let tools = catalog.list(Some(ManifestKind::Tool), None);
    assert_eq!(tools.len(), 2);

    let channels = catalog.list(Some(ManifestKind::Channel), None);
    assert_eq!(channels.len(), 1);
    Ok(())
}

#[test]
fn test_list_by_tag() -> anyhow::Result<()> {
    let tmp = tempfile::tempdir()?;
    create_test_registry(tmp.path())?;

    let catalog = RegistryCatalog::load(tmp.path())?;
    let defaults = catalog.list(None, Some("default"));
    assert_eq!(defaults.len(), 2);

    let messaging = catalog.list(None, Some("messaging"));
    assert_eq!(messaging.len(), 2); // slack (tool) and telegram (channel) both have "messaging" tag
    Ok(())
}

#[test]
fn test_get_by_name() -> anyhow::Result<()> {
    let tmp = tempfile::tempdir()?;
    create_test_registry(tmp.path())?;

    let catalog = RegistryCatalog::load(tmp.path())?;

    // Full key
    assert!(catalog.get("tools/slack").is_some());

    // Bare name
    assert!(catalog.get("slack").is_some());
    assert!(catalog.get("telegram").is_some());

    // Missing
    assert!(catalog.get("nonexistent").is_none());
    Ok(())
}

#[test]
fn test_search() -> anyhow::Result<()> {
    let tmp = tempfile::tempdir()?;
    create_test_registry(tmp.path())?;

    let catalog = RegistryCatalog::load(tmp.path())?;

    let results = catalog.search("slack");
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].name, "slack");

    let results = catalog.search("messaging");
    assert!(!results.is_empty());

    let results = catalog.search("nonexistent query");
    assert!(results.is_empty());
    Ok(())
}

#[test]
fn test_resolve_bundle() -> anyhow::Result<()> {
    let tmp = tempfile::tempdir()?;
    create_test_registry(tmp.path())?;

    let catalog = RegistryCatalog::load(tmp.path())?;

    let (manifests, missing) = catalog.resolve_bundle("default")?;
    assert_eq!(manifests.len(), 3);
    assert!(missing.is_empty());

    assert!(catalog.resolve_bundle("nonexistent").is_err());
    Ok(())
}

#[test]
fn test_resolve_single_or_bundle() -> anyhow::Result<()> {
    let tmp = tempfile::tempdir()?;
    create_test_registry(tmp.path())?;

    let catalog = RegistryCatalog::load(tmp.path())?;

    // Single extension
    let (manifests, bundle) = catalog.resolve("slack")?;
    assert_eq!(manifests.len(), 1);
    assert!(bundle.is_none());

    // Bundle
    let (manifests, bundle) = catalog.resolve("default")?;
    assert_eq!(manifests.len(), 3);
    assert!(bundle.is_some());
    Ok(())
}

#[test]
fn test_bundle_names() -> anyhow::Result<()> {
    let tmp = tempfile::tempdir()?;
    create_test_registry(tmp.path())?;

    let catalog = RegistryCatalog::load(tmp.path())?;
    let names = catalog.bundle_names();
    assert_eq!(names, vec!["default", "messaging"]);
    Ok(())
}

#[test]
fn test_directory_not_found() {
    let result = RegistryCatalog::load(Path::new("/nonexistent/path"));
    assert!(result.is_err());
}

#[test]
fn test_load_or_embedded_succeeds() {
    // Should always succeed: either finds registry/ on disk or falls back to embedded
    let catalog = RegistryCatalog::load_or_embedded()
        .expect("registry should load from disk or embedded fallback");
    // At minimum, the embedded catalog from the repo should have entries
    assert!(!catalog.all().is_empty() || !catalog.bundle_names().is_empty());
}

#[test]
fn test_bundle_entries_resolve_against_real_registry() {
    // Load the actual registry/ directory (catches stale bundle refs after renames)
    let catalog = RegistryCatalog::load_or_embedded()
        .expect("registry should load from disk or embedded fallback");

    for bundle_name in catalog.bundle_names() {
        let (manifests, missing) = catalog
            .resolve_bundle(bundle_name)
            .expect("bundle should resolve");
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
