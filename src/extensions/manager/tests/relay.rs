//! Tests for relay channel detection, removal, and installed-kind checks.

use std::sync::Arc;

use super::make_manager_custom_dirs;

// ── resolve_env_credentials tests ────────────────────────────────────

#[tokio::test]
async fn test_determine_installed_kind_does_not_auto_install_relay() {
    // Regression: determine_installed_kind used to auto-insert into
    // installed_relay_extensions when a ChannelRelay registry entry existed,
    // even though the user never installed it. It should be read-only.
    let dir = tempfile::tempdir().expect("temp dir");
    let mgr = make_manager_custom_dirs(dir.path().to_path_buf(), dir.path().to_path_buf());

    // The manager has no relay extensions installed
    assert!(
        mgr.installed_relay_extensions.read().await.is_empty(),
        "Should start with no installed relay extensions"
    );

    // Calling determine_installed_kind for a non-installed name returns NotInstalled
    let result = mgr.determine_installed_kind("slack-relay").await;
    assert!(result.is_err(), "Should return NotInstalled");

    // Crucially: installed_relay_extensions must still be empty
    assert!(
        mgr.installed_relay_extensions.read().await.is_empty(),
        "determine_installed_kind must not modify installed_relay_extensions"
    );
}

#[tokio::test]
async fn test_is_relay_channel_detects_stored_token() {
    let dir = tempfile::tempdir().expect("temp dir");
    let mgr = make_manager_custom_dirs(dir.path().to_path_buf(), dir.path().to_path_buf());

    // No token stored → not a relay channel
    assert!(!mgr.is_relay_channel("slack-relay").await);

    // Store a stream token
    mgr.secrets
        .create(
            "test",
            crate::secrets::CreateSecretParams::new("relay:slack-relay:stream_token", "tok123"),
        )
        .await
        .expect("store token");

    // Now it's detected as a relay channel
    assert!(mgr.is_relay_channel("slack-relay").await);
}

#[tokio::test]
async fn test_remove_relay_shuts_down_via_relay_channel_manager() {
    // Regression: remove() only checked channel_runtime for shutdown, missing
    // relay-only mode where only relay_channel_manager is set.
    let dir = tempfile::tempdir().expect("temp dir");
    let mgr = make_manager_custom_dirs(dir.path().to_path_buf(), dir.path().to_path_buf());

    // Set up relay channel manager with a stub channel
    let cm = Arc::new(crate::channels::ChannelManager::new());
    let (stub, _tx) = crate::testing::StubChannel::new("slack-relay");
    cm.add(Box::new(stub)).await;
    mgr.set_relay_channel_manager(Arc::clone(&cm)).await;

    // Mark as installed + store a token so determine_installed_kind finds it
    mgr.installed_relay_extensions
        .write()
        .await
        .insert("slack-relay".to_string());
    mgr.secrets
        .create(
            "test",
            crate::secrets::CreateSecretParams::new("relay:slack-relay:stream_token", "tok123"),
        )
        .await
        .expect("store token");

    // Verify channel exists before removal
    assert!(cm.get_channel("slack-relay").await.is_some());

    // Remove should succeed and shut down the channel
    let result = mgr.remove("slack-relay").await;
    assert!(result.is_ok(), "remove should succeed: {:?}", result.err());

    // installed_relay_extensions should be cleared
    assert!(
        !mgr.installed_relay_extensions
            .read()
            .await
            .contains("slack-relay"),
        "Should be removed from installed set"
    );
}
