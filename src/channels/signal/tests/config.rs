//! Tests for Signal channel construction, config normalization, and the
//! debug-mode toggle.

use super::*;

#[test]
fn creates_with_correct_fields() {
    let ch = make_channel();
    assert_eq!(ch.config.http_url, "http://127.0.0.1:8686");
    assert_eq!(ch.config.account, "+1234567890");
    assert_eq!(ch.config.allow_from.len(), 1);
    assert!(ch.config.allow_from_groups.is_empty());
    assert!(!ch.config.ignore_attachments);
    assert!(!ch.config.ignore_stories);
}

#[test]
fn strips_trailing_slash() {
    let mut config = make_config();
    config.http_url = "http://127.0.0.1:8686/".to_string();
    let ch = make_channel_from_config(config);
    assert_eq!(ch.config.http_url, "http://127.0.0.1:8686");
}

#[test]
fn debug_mode_disabled_by_default() {
    let ch = make_channel();
    assert!(!ch.is_debug());
}

#[test]
fn debug_mode_toggle() {
    let ch = make_channel();

    // Initially disabled
    assert!(!ch.is_debug());

    // Toggle on
    let new_state = ch.toggle_debug();
    assert!(new_state);
    assert!(ch.is_debug());

    // Toggle off
    let new_state = ch.toggle_debug();
    assert!(!new_state);
    assert!(!ch.is_debug());
}

#[test]
fn debug_mode_persists_across_toggles() {
    let ch = make_channel();

    // Multiple toggles
    ch.toggle_debug();
    assert!(ch.is_debug());
    ch.toggle_debug();
    assert!(!ch.is_debug());
    ch.toggle_debug();
    assert!(ch.is_debug());
    ch.toggle_debug();
    assert!(!ch.is_debug());
}

#[test]
fn name_returns_signal() {
    let ch = make_channel();
    assert_eq!(ch.name(), "signal");
}

// ── trailing slash variations ───────────────────────────────────

#[test]
fn strips_multiple_trailing_slashes() {
    let mut config = make_config();
    config.http_url = "http://127.0.0.1:8686///".to_string();
    let ch = make_channel_from_config(config);
    assert_eq!(ch.config.http_url, "http://127.0.0.1:8686");
}

#[test]
fn preserves_url_without_trailing_slash() {
    let config = make_config();
    let ch = make_channel_from_config(config);
    assert_eq!(ch.config.http_url, "http://127.0.0.1:8686");
}
