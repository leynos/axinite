//! Tests for workspace writes, namespacing, and the channel workspace store.

use std::sync::Arc;

use crate::channels::wasm::capabilities::ChannelCapabilities;
use crate::channels::wasm::host::message::PendingWorkspaceWrite;
use crate::channels::wasm::host::{ChannelHostState, ChannelWorkspaceStore};
use crate::tools::wasm::{WorkspaceCapability, WorkspaceReader};

#[test]
fn test_workspace_write_prefixing() {
    let caps = ChannelCapabilities::for_channel("slack");
    let mut state = ChannelHostState::new("slack", caps);

    state
        .workspace_write("state.json", "{}".to_string())
        .unwrap();

    let writes = state.take_pending_writes();
    assert_eq!(writes.len(), 1);
    assert_eq!(writes[0].path, "channels/slack/state.json");
}

#[test]
fn test_workspace_write_path_traversal_blocked() {
    let caps = ChannelCapabilities::for_channel("slack");
    let mut state = ChannelHostState::new("slack", caps);

    // Try to escape namespace
    let result = state.workspace_write("../secrets.json", "{}".to_string());
    assert!(result.is_err());

    // Absolute path
    let result = state.workspace_write("/etc/passwd", "{}".to_string());
    assert!(result.is_err());
}

#[test]
fn test_channel_workspace_store_commit_and_read() {
    let store = ChannelWorkspaceStore::new();

    // Initially empty
    assert!(store.read("channels/telegram/offset").is_none());

    // Commit some writes
    let writes = vec![
        PendingWorkspaceWrite {
            path: "channels/telegram/offset".to_string(),
            content: "103".to_string(),
        },
        PendingWorkspaceWrite {
            path: "channels/telegram/state.json".to_string(),
            content: r#"{"ok":true}"#.to_string(),
        },
    ];
    store.commit_writes(&writes);

    // Should be readable
    assert_eq!(
        store.read("channels/telegram/offset"),
        Some("103".to_string())
    );
    assert_eq!(
        store.read("channels/telegram/state.json"),
        Some(r#"{"ok":true}"#.to_string())
    );

    // Overwrite a value
    let writes2 = vec![PendingWorkspaceWrite {
        path: "channels/telegram/offset".to_string(),
        content: "200".to_string(),
    }];
    store.commit_writes(&writes2);
    assert_eq!(
        store.read("channels/telegram/offset"),
        Some("200".to_string())
    );

    // Empty writes are a no-op
    store.commit_writes(&[]);
    assert_eq!(
        store.read("channels/telegram/offset"),
        Some("200".to_string())
    );
}

// === QA Plan P2 - 2.3: WASM channel lifecycle tests ===

#[test]
fn test_workspace_write_then_read_round_trip() {
    // Full lifecycle: write in one "callback", commit, then read in a
    // subsequent "callback" using the same store as the workspace reader.
    let store = Arc::new(ChannelWorkspaceStore::new());

    // --- Callback 1: write workspace data ---
    let caps = ChannelCapabilities::for_channel("telegram");
    let mut state = ChannelHostState::new("telegram", caps);

    state
        .workspace_write("offset", "12345".to_string())
        .unwrap();
    state
        .workspace_write("state.json", r#"{"ok":true}"#.to_string())
        .unwrap();

    let writes = state.take_pending_writes();
    assert_eq!(writes.len(), 2);
    store.commit_writes(&writes);

    // --- Callback 2: read back the data written in callback 1 ---
    // Build capabilities with the store as the workspace reader.
    let mut caps2 = ChannelCapabilities::for_channel("telegram");
    caps2.tool_capabilities.workspace_read = Some(WorkspaceCapability {
        allowed_prefixes: vec![], // empty = all paths allowed
        reader: Some(Arc::clone(&store) as Arc<dyn WorkspaceReader>),
    });
    let state2 = ChannelHostState::new("telegram", caps2);

    // workspace_read prefixes path with "channels/telegram/" before delegating.
    let offset = state2.workspace_read("offset").unwrap();
    assert_eq!(offset, Some("12345".to_string()));

    let json = state2.workspace_read("state.json").unwrap();
    assert_eq!(json, Some(r#"{"ok":true}"#.to_string()));

    // Non-existent key returns None.
    let missing = state2.workspace_read("no_such_key").unwrap();
    assert!(missing.is_none());
}

#[test]
fn test_workspace_overwrite_across_callbacks() {
    // Verify that a second write to the same key overwrites the first.
    let store = Arc::new(ChannelWorkspaceStore::new());

    // Callback 1: write initial value.
    let caps = ChannelCapabilities::for_channel("slack");
    let mut state = ChannelHostState::new("slack", caps);
    state.workspace_write("cursor", "100".to_string()).unwrap();
    let writes = state.take_pending_writes();
    store.commit_writes(&writes);

    // Callback 2: overwrite the same key.
    let caps2 = ChannelCapabilities::for_channel("slack");
    let mut state2 = ChannelHostState::new("slack", caps2);
    state2.workspace_write("cursor", "200".to_string()).unwrap();
    let writes2 = state2.take_pending_writes();
    store.commit_writes(&writes2);

    // Callback 3: read back -- should see the overwritten value.
    let mut caps3 = ChannelCapabilities::for_channel("slack");
    caps3.tool_capabilities.workspace_read = Some(WorkspaceCapability {
        allowed_prefixes: vec![],
        reader: Some(Arc::clone(&store) as Arc<dyn WorkspaceReader>),
    });
    let state3 = ChannelHostState::new("slack", caps3);

    let value = state3.workspace_read("cursor").unwrap();
    assert_eq!(value, Some("200".to_string()));
}

#[test]
fn test_channels_have_isolated_namespaces() {
    // Two channels writing to the same relative path should not collide.
    let store = Arc::new(ChannelWorkspaceStore::new());

    // Telegram writes "offset" = "100".
    let caps_tg = ChannelCapabilities::for_channel("telegram");
    let mut state_tg = ChannelHostState::new("telegram", caps_tg);
    state_tg
        .workspace_write("offset", "100".to_string())
        .unwrap();
    store.commit_writes(&state_tg.take_pending_writes());

    // Slack writes "offset" = "200".
    let caps_sl = ChannelCapabilities::for_channel("slack");
    let mut state_sl = ChannelHostState::new("slack", caps_sl);
    state_sl
        .workspace_write("offset", "200".to_string())
        .unwrap();
    store.commit_writes(&state_sl.take_pending_writes());

    // Reading back: each channel sees its own value.
    let mut caps_tg_read = ChannelCapabilities::for_channel("telegram");
    caps_tg_read.tool_capabilities.workspace_read = Some(WorkspaceCapability {
        allowed_prefixes: vec![],
        reader: Some(Arc::clone(&store) as Arc<dyn WorkspaceReader>),
    });
    let tg_reader = ChannelHostState::new("telegram", caps_tg_read);
    assert_eq!(
        tg_reader.workspace_read("offset").unwrap(),
        Some("100".to_string())
    );

    let mut caps_sl_read = ChannelCapabilities::for_channel("slack");
    caps_sl_read.tool_capabilities.workspace_read = Some(WorkspaceCapability {
        allowed_prefixes: vec![],
        reader: Some(Arc::clone(&store) as Arc<dyn WorkspaceReader>),
    });
    let sl_reader = ChannelHostState::new("slack", caps_sl_read);
    assert_eq!(
        sl_reader.workspace_read("offset").unwrap(),
        Some("200".to_string())
    );
}
