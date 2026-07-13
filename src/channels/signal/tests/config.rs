//! Tests for Signal channel construction, config normalization, and the
//! debug-mode toggle.

use super::*;

#[test]
fn creates_with_correct_fields() -> Result<(), ChannelError> {
    let ch = make_channel()?;
    assert_eq!(ch.config.http_url, "http://127.0.0.1:8686");
    assert_eq!(ch.config.account, "+1234567890");
    assert_eq!(ch.config.allow_from.len(), 1);
    assert!(ch.config.allow_from_groups.is_empty());
    assert!(!ch.config.ignore_attachments);
    assert!(!ch.config.ignore_stories);
    Ok(())
}

#[test]
fn strips_trailing_slash() -> Result<(), ChannelError> {
    let mut config = make_config();
    config.http_url = "http://127.0.0.1:8686/".to_string();
    let ch = SignalChannel::new(config)?;
    assert_eq!(ch.config.http_url, "http://127.0.0.1:8686");
    Ok(())
}

#[test]
fn debug_mode_disabled_by_default() -> Result<(), ChannelError> {
    let ch = make_channel()?;
    assert!(!ch.is_debug());
    Ok(())
}

#[test]
fn debug_mode_toggle() -> Result<(), ChannelError> {
    let ch = make_channel()?;

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

    Ok(())
}

#[test]
fn debug_mode_persists_across_toggles() -> Result<(), ChannelError> {
    let ch = make_channel()?;

    // Multiple toggles
    ch.toggle_debug();
    assert!(ch.is_debug());
    ch.toggle_debug();
    assert!(!ch.is_debug());
    ch.toggle_debug();
    assert!(ch.is_debug());
    ch.toggle_debug();
    assert!(!ch.is_debug());

    Ok(())
}

#[test]
fn name_returns_signal() -> Result<(), ChannelError> {
    let ch = make_channel()?;
    assert_eq!(ch.name(), "signal");
    Ok(())
}

// ── trailing slash variations ───────────────────────────────────

#[test]
fn strips_multiple_trailing_slashes() -> Result<(), ChannelError> {
    let mut config = make_config();
    config.http_url = "http://127.0.0.1:8686///".to_string();
    let ch = SignalChannel::new(config)?;
    assert_eq!(ch.config.http_url, "http://127.0.0.1:8686");
    Ok(())
}

#[test]
fn preserves_url_without_trailing_slash() -> Result<(), ChannelError> {
    let config = make_config();
    let ch = SignalChannel::new(config)?;
    assert_eq!(ch.config.http_url, "http://127.0.0.1:8686");
    Ok(())
}
